import { useState, useEffect, useCallback, useRef } from 'react';
import { Change, Diff, Review, fetchChanges, fetchDiff, fetchReview, createReview, replyToThread, resolveThread } from './api';
import { DiffViewer, DiffViewerHandle } from './components/DiffViewer';
import { ChangeList } from './components/ChangeList';
import { CommentPanel } from './components/CommentPanel';

type FocusedPanel = 'changes' | 'diff' | 'threads';

export default function App() {
  const [changes, setChanges] = useState<Change[]>([]);
  const [selectedChange, setSelectedChange] = useState<Change | null>(null);
  const [diff, setDiff] = useState<Diff | null>(null);
  const [review, setReview] = useState<Review | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [focusedPanel, setFocusedPanel] = useState<FocusedPanel>('changes');
  const [selectedThreadId, setSelectedThreadId] = useState<string | null>(null);
  const [replyingToThread, setReplyingToThread] = useState(false);
  const [replyText, setReplyText] = useState('');
  const [submittingReply, setSubmittingReply] = useState(false);

  const diffViewerRef = useRef<DiffViewerHandle>(null);

  useEffect(() => {
    fetchChanges()
      .then(setChanges)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    if (!selectedChange) {
      setDiff(null);
      setReview(null);
      setSelectedThreadId(null);
      return;
    }

    setLoading(true);
    Promise.all([
      fetchDiff(selectedChange.change_id),
      fetchReview(selectedChange.change_id),
    ])
      .then(async ([d, r]) => {
        setDiff(d);
        // Auto-create review if it doesn't exist
        if (!r) {
          const newReview = await createReview(selectedChange.change_id);
          setReview(newReview);
        } else {
          setReview(r);
        }
      })
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, [selectedChange]);

  // Get thread indices for navigation
  const threadIds = review?.threads.map(t => t.id) || [];

  // Keyboard navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLInputElement) return;

      // Tab cycles through panels: changes -> diff -> threads -> changes
      // Shift+Tab goes backwards
      if (e.key === 'Tab') {
        e.preventDefault();
        if (e.shiftKey) {
          setFocusedPanel((p) => {
            if (p === 'changes') return review?.threads.length ? 'threads' : 'diff';
            if (p === 'diff') return 'changes';
            return 'diff';
          });
        } else {
          setFocusedPanel((p) => {
            if (p === 'changes') return 'diff';
            if (p === 'diff') return review?.threads.length ? 'threads' : 'changes';
            return 'changes';
          });
        }
        return;
      }

      // j/k for change list when focused on changes panel
      if (focusedPanel === 'changes' && changes.length > 0) {
        const currentIdx = selectedChange
          ? changes.findIndex((c) => c.change_id === selectedChange.change_id)
          : -1;

        switch (e.key) {
          case 'j':
          case 'ArrowDown': {
            e.preventDefault();
            const nextIdx = currentIdx < 0 || currentIdx >= changes.length - 1 ? 0 : currentIdx + 1;
            setSelectedChange(changes[nextIdx]);
            break;
          }
          case 'k':
          case 'ArrowUp': {
            e.preventDefault();
            const prevIdx = currentIdx <= 0 ? changes.length - 1 : currentIdx - 1;
            setSelectedChange(changes[prevIdx]);
            break;
          }
        }
      }

      // j/k for threads when focused on threads panel
      if (focusedPanel === 'threads' && threadIds.length > 0) {
        const currentIdx = selectedThreadId ? threadIds.indexOf(selectedThreadId) : -1;

        switch (e.key) {
          case 'j':
          case 'ArrowDown': {
            e.preventDefault();
            const nextIdx = currentIdx < 0 || currentIdx >= threadIds.length - 1 ? 0 : currentIdx + 1;
            setSelectedThreadId(threadIds[nextIdx]);
            break;
          }
          case 'k':
          case 'ArrowUp': {
            e.preventDefault();
            const prevIdx = currentIdx <= 0 ? threadIds.length - 1 : currentIdx - 1;
            setSelectedThreadId(threadIds[prevIdx]);
            break;
          }
          case 'x': {
            // Resolve thread
            if (selectedThreadId && review) {
              e.preventDefault();
              handleResolveThread(selectedThreadId);
            }
            break;
          }
          case 'r': {
            // Focus reply input (handled by CommentPanel)
            break;
          }
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [changes, selectedChange, focusedPanel, threadIds, selectedThreadId, review]);

  const handleReviewUpdate = useCallback((updatedReview: Review) => {
    setReview(updatedReview);
  }, []);

  const handleThreadSelect = useCallback((threadId: string | null) => {
    setSelectedThreadId(threadId);
    setReplyingToThread(false);
    setReplyText('');
  }, []);

  const handleStartReply = useCallback((threadId: string) => {
    setSelectedThreadId(threadId);
    setReplyingToThread(true);
  }, []);

  const handleReplySubmit = useCallback(async (threadId: string) => {
    if (!replyText.trim() || !review) return;

    setSubmittingReply(true);
    try {
      const updatedReview = await replyToThread(review.change_id, threadId, replyText.trim());
      setReview(updatedReview);
      setReplyText('');
      setReplyingToThread(false);
    } catch (e) {
      console.error('Failed to reply:', e);
    } finally {
      setSubmittingReply(false);
    }
  }, [replyText, review]);

  const handleCancelReply = useCallback(() => {
    setReplyingToThread(false);
    setReplyText('');
  }, []);

  const handleResolveThread = useCallback(async (threadId: string) => {
    if (!review) return;

    try {
      const updatedReview = await resolveThread(review.change_id, threadId);
      setReview(updatedReview);
    } catch (e) {
      console.error('Failed to resolve:', e);
    }
  }, [review]);

  if (error) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div className="bg-red-100 text-red-800 p-4 rounded-lg border border-red-200">
          Error: {error}
          <button
            onClick={() => setError(null)}
            className="ml-4 underline"
          >
            Dismiss
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="h-screen flex flex-col overflow-hidden">

      <div className="flex flex-1 overflow-hidden">
        {/* Change list sidebar */}
        <aside className="w-80 border-r border-gray-200 overflow-y-auto bg-gray-50">
          <ChangeList
            changes={changes}
            selectedId={selectedChange?.change_id}
            onSelect={setSelectedChange}
            loading={loading && changes.length === 0}
            focused={focusedPanel === 'changes'}
          />
        </aside>

        {/* Main content */}
        <main className="flex-1 flex flex-col overflow-hidden">
          {selectedChange && diff ? (
            <>
              <div className="bg-gray-100 border-b border-gray-200 p-4">
                <h2 className="font-mono text-sm text-gray-500">
                  {selectedChange.change_id.slice(0, 12)}
                </h2>
                <p className="text-lg">{selectedChange.description || '(no description)'}</p>
              </div>

              <div className="flex-1 flex overflow-hidden">
                <div className="flex-1 overflow-auto">
                  <DiffViewer
                    ref={diffViewerRef}
                    diff={diff}
                    review={review}
                    onReviewUpdate={handleReviewUpdate}
                    focused={focusedPanel === 'diff'}
                    replyingToThread={replyingToThread && focusedPanel === 'diff'}
                    onStartReply={handleStartReply}
                    onReplySubmit={handleReplySubmit}
                    onCancelReply={handleCancelReply}
                    onResolveThread={handleResolveThread}
                    replyText={replyText}
                    onReplyTextChange={setReplyText}
                    submittingReply={submittingReply}
                  />
                </div>

                {review && (
                  <aside className="w-96 border-l border-gray-200 overflow-y-auto bg-gray-50">
                    <CommentPanel
                      review={review}
                      onReviewUpdate={handleReviewUpdate}
                      focused={focusedPanel === 'threads'}
                      selectedThreadId={selectedThreadId}
                      onThreadSelect={handleThreadSelect}
                      replyingToThread={replyingToThread}
                      onStartReply={handleStartReply}
                      onReplySubmit={handleReplySubmit}
                      onCancelReply={handleCancelReply}
                      onResolveThread={handleResolveThread}
                      replyText={replyText}
                      onReplyTextChange={setReplyText}
                      submittingReply={submittingReply}
                    />
                  </aside>
                )}
              </div>
            </>
          ) : (
            <div className="flex-1 flex items-center justify-center text-gray-400">
              {loading ? 'Loading...' : 'Select a change to view'}
            </div>
          )}
        </main>
      </div>
    </div>
  );
}
