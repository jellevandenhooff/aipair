import { Suspense, useEffect, useMemo } from 'react';
import { ChangeList } from './components/ChangeList';
import { SelectedChangeView } from './components/SelectedChangeView';
import { TodoPanel } from './components/TodoPanel';
import { SessionsPanel } from './components/SessionsPanel';
import { TimelineView } from './components/TimelineView';
import { ErrorBoundary } from './components/ErrorBoundary';
import { useAppContext } from './context';
import { useChanges } from './hooks';

// Inner component that needs changes data to find selected change
function MainContent() {
  const { changes } = useChanges();
  const {
    selectedChangeId,
    selectChange,
    focusedPanel,
    cyclePanel,
    navigateChanges,
  } = useAppContext();

  // Find the full change object for the selected ID
  const selectedChange = useMemo(
    () => changes.find((c) => c.change_id === selectedChangeId) ?? null,
    [changes, selectedChangeId]
  );

  // Keyboard navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLInputElement) return;

      // Tab cycles through panels
      if (e.key === 'Tab') {
        e.preventDefault();
        // We don't have direct access to thread count here, so assume threads exist if change selected
        cyclePanel(e.shiftKey, selectedChangeId !== null);
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
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [focusedPanel, changes, cyclePanel, navigateChanges, selectChange, selectedChangeId]);

  return (
    <main className="flex-1 flex flex-col overflow-hidden relative">
      {selectedChange ? (
        <Suspense fallback={<LoadingView message="Loading diff..." />}>
          <SelectedChangeView change={selectedChange} />
        </Suspense>
      ) : (
        <div className="flex-1 flex items-center justify-center text-gray-400">
          Select a change to view
        </div>
      )}
    </main>
  );
}

function LoadingView({ message }: { message: string }) {
  return (
    <div className="flex-1 flex items-center justify-center text-gray-400">
      {message}
    </div>
  );
}

export default function App() {
  const isTimeline = window.location.pathname === '/timeline';
  const { setFocusedPanel, isSelectingChange, todoPanelVisible, toggleTodoPanel, sessionsPanelVisible, toggleSessionsPanel } = useAppContext();

  // Global backtick toggle for todo panel, ~ for sessions panel
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLInputElement) return;
      if (e.key === '`') {
        e.preventDefault();
        toggleTodoPanel();
      } else if (e.key === '~') {
        e.preventDefault();
        toggleSessionsPanel();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [toggleTodoPanel, toggleSessionsPanel]);

  if (isTimeline) {
    return <TimelineView />;
  }

  return (
    <ErrorBoundary>
      <div className="h-screen flex flex-col overflow-hidden">
        {/* Full-width loading bar with 500ms delay */}
        {isSelectingChange && (
          <div
            className="fixed top-0 left-0 right-0 h-1 bg-blue-100 overflow-hidden z-50 animate-fade-in-delayed"
            style={{
              animation: 'fadeIn 0.15s ease-in 0.5s forwards',
              opacity: 0,
            }}
          >
            <div className="h-full bg-blue-500 animate-pulse w-full" />
          </div>
        )}
        <div className="flex flex-1 overflow-hidden">
          {/* Change list sidebar */}
          <aside
            className="w-80 border-r border-gray-200 overflow-y-auto bg-gray-50"
            onClick={() => setFocusedPanel('changes')}
          >
            <Suspense fallback={<LoadingView message="Loading changes..." />}>
              <ChangeList />
            </Suspense>
          </aside>

          {/* Main content - also wrapped in Suspense for change data */}
          <Suspense fallback={<LoadingView message="Loading..." />}>
            <MainContent />
          </Suspense>
        </div>

        {/* Todo panel (collapsible bottom panel) */}
        {todoPanelVisible && (
          <Suspense fallback={<LoadingView message="Loading todos..." />}>
            <TodoPanel />
          </Suspense>
        )}

        {/* Sessions panel (collapsible bottom panel) */}
        {sessionsPanelVisible && (
          <Suspense fallback={<LoadingView message="Loading sessions..." />}>
            <SessionsPanel />
          </Suspense>
        )}
      </div>
    </ErrorBoundary>
  );
}
