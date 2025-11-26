import { useEffect, useRef, useMemo, useLayoutEffect } from 'react';
import { DiffViewer, DiffViewerHandle } from './components/DiffViewer';
import { ChangeList } from './components/ChangeList';
import { CommentPanel } from './components/CommentPanel';
import { useAppStore } from './store';
import { useChanges, useDiff, useReview } from './hooks';

export default function App() {
  // UI state from Zustand
  const selectedChangeId = useAppStore((s) => s.selectedChangeId);
  const selectedRevision = useAppStore((s) => s.selectedRevision);
  const comparisonBase = useAppStore((s) => s.comparisonBase);
  const focusedPanel = useAppStore((s) => s.focusedPanel);

  const selectChange = useAppStore((s) => s.selectChange);
  const selectRevision = useAppStore((s) => s.selectRevision);
  const setFocusedPanel = useAppStore((s) => s.setFocusedPanel);
  const cyclePanel = useAppStore((s) => s.cyclePanel);
  const navigateChanges = useAppStore((s) => s.navigateChanges);
  const navigateThreads = useAppStore((s) => s.navigateThreads);

  // Data from SWR
  const { data: changes = [], error: changesError, isLoading: changesLoading } = useChanges();
  const { data: diffResponse, error: diffError } = useDiff(
    selectedChangeId,
    selectedRevision?.commit_id,
    comparisonBase?.commit_id
  );
  const { data: review } = useReview(selectedChangeId);

  // Find the full change object for the selected ID
  const selectedChange = useMemo(
    () => changes.find((c) => c.change_id === selectedChangeId) ?? null,
    [changes, selectedChangeId]
  );

  const diffViewerRef = useRef<DiffViewerHandle>(null);
  const lastReviewChangeIdRef = useRef<string | null>(null);

  // Sync selectedRevision with review data - when a new review loads, set revision to latest
  // Using useLayoutEffect to update before paint, avoiding flash
  useLayoutEffect(() => {
    if (!review) return;

    // Only update when the review's change_id changes (new change selected)
    if (lastReviewChangeIdRef.current === review.change_id) return;
    lastReviewChangeIdRef.current = review.change_id;

    // Set selectedRevision to the latest revision
    if (review.revisions.length > 0) {
      const latestRevision = review.revisions[review.revisions.length - 1];
      selectRevision(latestRevision);
    }
  }, [review, selectRevision]);

  // Keyboard navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLInputElement) return;

      const hasThreads = (review?.threads.length ?? 0) > 0;

      // Tab cycles through panels
      if (e.key === 'Tab') {
        e.preventDefault();
        cyclePanel(e.shiftKey, hasThreads);
        return;
      }

      // j/k for change list when focused on changes panel
      if (focusedPanel === 'changes' && changes.length > 0) {
        if (e.key === 'j' || e.key === 'ArrowDown') {
          e.preventDefault();
          navigateChanges('down', changes, selectChange);
        } else if (e.key === 'k' || e.key === 'ArrowUp') {
          e.preventDefault();
          navigateChanges('up', changes, selectChange);
        }
      }

      // j/k for threads when focused on threads panel
      if (focusedPanel === 'threads' && review) {
        const threadIds = review.threads.map((t) => t.id);

        if (e.key === 'j' || e.key === 'ArrowDown') {
          e.preventDefault();
          navigateThreads('down', threadIds);
        } else if (e.key === 'k' || e.key === 'ArrowUp') {
          e.preventDefault();
          navigateThreads('up', threadIds);
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [focusedPanel, changes, review, cyclePanel, navigateChanges, navigateThreads, selectChange]);

  const error = changesError || diffError;
  if (error) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div className="bg-red-100 text-red-800 p-4 rounded-lg border border-red-200">
          Error: {(error as Error).message}
        </div>
      </div>
    );
  }

  return (
    <div className="h-screen flex flex-col overflow-hidden">
      <div className="flex flex-1 overflow-hidden">
        {/* Change list sidebar */}
        <aside
          className="w-80 border-r border-gray-200 overflow-y-auto bg-gray-50"
          onClick={() => setFocusedPanel('changes')}
        >
          <ChangeList
            changes={changes}
            selectedChangeId={selectedChangeId}
            onSelectChange={selectChange}
            loading={changesLoading}
          />
        </aside>

        {/* Main content */}
        <main className="flex-1 flex flex-col overflow-hidden">
          {selectedChange && diffResponse ? (
            <>
              <div className="flex-1 flex overflow-hidden">
                <div
                  className="flex-1 overflow-auto"
                  onClick={() => setFocusedPanel('diff')}
                >
                  <DiffViewer
                    ref={diffViewerRef}
                    diff={diffResponse.diff}
                    targetMessage={diffResponse.target_message}
                    messageDiff={diffResponse.message_diff}
                    review={review ?? null}
                    changeId={selectedChangeId!}
                    description={selectedChange?.description}
                  />
                </div>

                {review && (
                  <aside
                    className="w-96 border-l border-gray-200 overflow-y-auto bg-gray-50"
                    onClick={() => setFocusedPanel('threads')}
                  >
                    <CommentPanel
                      review={review}
                      selectedChange={selectedChange}
                    />
                  </aside>
                )}
              </div>
            </>
          ) : (
            <div className="flex-1 flex items-center justify-center text-gray-400">
              {changesLoading ? 'Loading...' : 'Select a change to view'}
            </div>
          )}
        </main>
      </div>
    </div>
  );
}
