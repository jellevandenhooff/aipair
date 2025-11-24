import { useState, useEffect, useCallback } from 'react';
import { Change, Diff, Review, fetchChanges, fetchDiff, fetchReview, createReview } from './api';
import { DiffViewer } from './components/DiffViewer';
import { ChangeList } from './components/ChangeList';
import { CommentPanel } from './components/CommentPanel';

export default function App() {
  const [changes, setChanges] = useState<Change[]>([]);
  const [selectedChange, setSelectedChange] = useState<Change | null>(null);
  const [diff, setDiff] = useState<Diff | null>(null);
  const [review, setReview] = useState<Review | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [focusedPanel, setFocusedPanel] = useState<'changes' | 'diff'>('changes');

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
      return;
    }

    setLoading(true);
    Promise.all([
      fetchDiff(selectedChange.change_id),
      fetchReview(selectedChange.change_id),
    ])
      .then(([d, r]) => {
        setDiff(d);
        setReview(r);
      })
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, [selectedChange]);

  // Keyboard navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLInputElement) return;

      // Tab switches focus between panels
      if (e.key === 'Tab') {
        e.preventDefault();
        setFocusedPanel((p) => (p === 'changes' ? 'diff' : 'changes'));
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
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [changes, selectedChange, focusedPanel]);

  const handleStartReview = useCallback(async () => {
    if (!selectedChange) return;
    try {
      const r = await createReview(selectedChange.change_id);
      setReview(r);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to create review');
    }
  }, [selectedChange]);

  const handleReviewUpdate = useCallback((updatedReview: Review) => {
    setReview(updatedReview);
  }, []);

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
    <div className="min-h-screen flex flex-col">
      <header className="bg-gray-100 border-b border-gray-200 p-4">
        <h1 className="text-xl font-bold">aipair</h1>
        <p className="text-sm text-gray-500">Code review for AI pair programming</p>
      </header>

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
              <div className="bg-gray-100 border-b border-gray-200 p-4 flex items-center justify-between">
                <div>
                  <h2 className="font-mono text-sm text-gray-500">
                    {selectedChange.change_id.slice(0, 12)}
                  </h2>
                  <p className="text-lg">{selectedChange.description || '(no description)'}</p>
                </div>
                {!review && (
                  <button
                    onClick={handleStartReview}
                    className="bg-blue-600 hover:bg-blue-700 text-white px-4 py-2 rounded"
                  >
                    Start Review
                  </button>
                )}
              </div>

              <div className="flex-1 flex overflow-hidden">
                <div className="flex-1 overflow-auto">
                  <DiffViewer
                    diff={diff}
                    review={review}
                    onReviewUpdate={handleReviewUpdate}
                    focused={focusedPanel === 'diff'}
                  />
                </div>

                {review && (
                  <aside className="w-96 border-l border-gray-200 overflow-y-auto bg-gray-50">
                    <CommentPanel
                      review={review}
                      onReviewUpdate={handleReviewUpdate}
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
