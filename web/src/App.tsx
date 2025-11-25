import { useEffect, useRef } from 'react';
import { DiffViewer, DiffViewerHandle } from './components/DiffViewer';
import { ChangeList } from './components/ChangeList';
import { CommentPanel } from './components/CommentPanel';
import { useAppStore } from './store';

export default function App() {
  const changes = useAppStore((s) => s.changes);
  const selectedChange = useAppStore((s) => s.selectedChange);
  const diff = useAppStore((s) => s.diff);
  const review = useAppStore((s) => s.review);
  const loading = useAppStore((s) => s.loading);
  const error = useAppStore((s) => s.error);
  const focusedPanel = useAppStore((s) => s.focusedPanel);

  const fetchChanges = useAppStore((s) => s.fetchChanges);
  const refreshData = useAppStore((s) => s.refreshData);
  const setFocusedPanel = useAppStore((s) => s.setFocusedPanel);
  const cyclePanel = useAppStore((s) => s.cyclePanel);
  const navigateChanges = useAppStore((s) => s.navigateChanges);
  const navigateThreads = useAppStore((s) => s.navigateThreads);
  const toggleThreadStatus = useAppStore((s) => s.toggleThreadStatus);
  const clearError = useAppStore((s) => s.clearError);

  const diffViewerRef = useRef<DiffViewerHandle>(null);

  // Initial fetch
  useEffect(() => {
    fetchChanges();
  }, [fetchChanges]);

  // Poll for updates every 3 seconds
  useEffect(() => {
    const interval = setInterval(() => {
      refreshData();
    }, 3000);
    return () => clearInterval(interval);
  }, [refreshData]);

  // Keyboard navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLInputElement) return;

      // Tab cycles through panels
      if (e.key === 'Tab') {
        e.preventDefault();
        cyclePanel(e.shiftKey);
        return;
      }

      // j/k for change list when focused on changes panel
      if (focusedPanel === 'changes' && changes.length > 0) {
        if (e.key === 'j' || e.key === 'ArrowDown') {
          e.preventDefault();
          navigateChanges('down');
        } else if (e.key === 'k' || e.key === 'ArrowUp') {
          e.preventDefault();
          navigateChanges('up');
        }
      }

      // j/k for threads when focused on threads panel
      if (focusedPanel === 'threads') {
        const { selectedThreadId } = useAppStore.getState();

        if (e.key === 'j' || e.key === 'ArrowDown') {
          e.preventDefault();
          navigateThreads('down');
        } else if (e.key === 'k' || e.key === 'ArrowUp') {
          e.preventDefault();
          navigateThreads('up');
        } else if (e.key === 'x' && selectedThreadId) {
          e.preventDefault();
          toggleThreadStatus(selectedThreadId);
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [focusedPanel, changes.length, cyclePanel, navigateChanges, navigateThreads, toggleThreadStatus]);

  if (error) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div className="bg-red-100 text-red-800 p-4 rounded-lg border border-red-200">
          Error: {error}
          <button onClick={clearError} className="ml-4 underline">
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
        <aside
          className="w-80 border-r border-gray-200 overflow-y-auto bg-gray-50"
          onClick={() => setFocusedPanel('changes')}
        >
          <ChangeList />
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
                <div
                  className="flex-1 overflow-auto"
                  onClick={() => setFocusedPanel('diff')}
                >
                  <DiffViewer ref={diffViewerRef} />
                </div>

                {review && (
                  <aside
                    className="w-96 border-l border-gray-200 overflow-y-auto bg-gray-50"
                    onClick={() => setFocusedPanel('threads')}
                  >
                    <CommentPanel />
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
