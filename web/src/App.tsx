import { Suspense, useEffect, useMemo } from 'react';
import { ChangeList } from './components/ChangeList';
import { SelectedChangeView } from './components/SelectedChangeView';
import { Terminal } from './components/Terminal';
import { TodoPanel } from './components/TodoPanel';
import { SessionSidebar } from './components/SessionSidebar';
import { TimelineView } from './components/TimelineView';
import { ErrorBoundary } from './components/ErrorBoundary';
import { useAppContext } from './context';
import { useChanges, useSessionChanges } from './hooks';

// Inner component that needs changes data to find selected change
function MainContent() {
  const { selectedSessionName, selectedSessionVersion, activeTab, setActiveTab } = useAppContext();
  const globalData = useChanges();

  // Convert version for API: UI shows pushes reversed (newest first), API uses 0-indexed from oldest
  const sessions = globalData.sessions;
  const apiVersion = useMemo(() => {
    if (selectedSessionVersion === 'live' || selectedSessionVersion === 'latest') return selectedSessionVersion;
    const reversedIdx = parseInt(selectedSessionVersion, 10);
    const session = sessions.find(s => s.name === selectedSessionName);
    if (!session || isNaN(reversedIdx)) return 'live';
    return String(session.pushes.length - 1 - reversedIdx);
  }, [selectedSessionVersion, sessions, selectedSessionName]);

  const sessionData = useSessionChanges(selectedSessionName, apiVersion);

  // Use session changes when a session is selected, otherwise global
  const changes = selectedSessionName
    ? (sessionData?.changes ?? [])
    : globalData.changes;

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

  const showTerminalTab = selectedSessionName !== null;

  return (
    <main className="flex-1 flex flex-col overflow-hidden relative">
      {/* Tab bar — only shown when a session is selected */}
      {showTerminalTab && (
        <div className="flex border-b border-gray-200 bg-white shrink-0">
          <button
            onClick={() => setActiveTab('review')}
            className={`px-4 py-1.5 text-sm font-medium border-b-2 transition-colors ${
              activeTab === 'review'
                ? 'border-blue-500 text-blue-600'
                : 'border-transparent text-gray-500 hover:text-gray-700'
            }`}
          >Review</button>
          <button
            onClick={() => setActiveTab('terminal')}
            className={`px-4 py-1.5 text-sm font-medium border-b-2 transition-colors ${
              activeTab === 'terminal'
                ? 'border-blue-500 text-blue-600'
                : 'border-transparent text-gray-500 hover:text-gray-700'
            }`}
          >Terminal</button>
        </div>
      )}

      {/* Tab content — both panels use absolute positioning so the terminal
           keeps its real dimensions (visibility:hidden, not display:none)
           to avoid 0x0 resize events that cause tmux newlines */}
      <div className="flex-1 relative overflow-hidden">
        {showTerminalTab && (
          <div className={`absolute inset-0 flex flex-col ${activeTab === 'terminal' ? '' : 'invisible'}`}>
            <Terminal sessionName={selectedSessionName} />
          </div>
        )}
        <div className={`absolute inset-0 flex flex-col overflow-hidden ${showTerminalTab && activeTab === 'terminal' ? 'invisible' : ''}`}>
          {selectedChange ? (
            <Suspense fallback={<LoadingView message="Loading diff..." />}>
              <SelectedChangeView change={selectedChange} />
            </Suspense>
          ) : (
            <div className="flex-1 flex items-center justify-center text-gray-400">
              Select a change to view
            </div>
          )}
        </div>
      </div>
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
  const { setFocusedPanel, isSelectingChange, todoPanelVisible, toggleTodoPanel } = useAppContext();

  // Global backtick toggle for todo panel
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLInputElement) return;
      if (e.key === '`') {
        e.preventDefault();
        toggleTodoPanel();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [toggleTodoPanel]);

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
          {/* Session sidebar */}
          <Suspense fallback={<LoadingView message="" />}>
            <SessionSidebar />
          </Suspense>

          {/* Change list */}
          <aside
            className="w-80 border-r border-gray-200 overflow-y-auto bg-gray-50"
            onClick={() => setFocusedPanel('changes')}
          >
            <Suspense fallback={<LoadingView message="Loading changes..." />}>
              <ChangeList />
            </Suspense>
          </aside>

          {/* Main content */}
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
      </div>
    </ErrorBoundary>
  );
}
