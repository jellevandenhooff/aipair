import { createContext, useContext, useState, useCallback, useTransition, ReactNode } from 'react';
import type { Change } from './api';

type FocusedPanel = 'changes' | 'diff' | 'threads' | 'todos';

// Selection state - triggers data fetching, needs transitions
interface SelectionState {
  selectedChangeId: string | null;
}

// UI state - instant updates, no transitions needed
interface UIState {
  focusedPanel: FocusedPanel;
  selectedThreadId: string | null;
  selectedTodoId: string | null;
  todoPanelVisible: boolean;
  selectedSessionName: string | null;
  selectedSessionVersion: string;
  newCommentText: string;
  replyingToThread: boolean;
  replyText: string;
  submittingReply: boolean;
}

interface AppContextValue {
  // Selection state + actions
  selectedChangeId: string | null;
  selectChange: (changeId: string | null) => void;
  isSelectingChange: boolean; // isPending from useTransition

  // UI state + actions
  focusedPanel: FocusedPanel;
  selectedThreadId: string | null;
  newCommentText: string;
  replyingToThread: boolean;
  replyText: string;
  submittingReply: boolean;
  setFocusedPanel: (panel: FocusedPanel) => void;
  cyclePanel: (reverse: boolean, hasThreads: boolean) => void;
  setSelectedThreadId: (id: string | null) => void;
  setNewCommentText: (text: string) => void;
  clearNewComment: () => void;
  startReply: (threadId: string) => void;
  cancelReply: () => void;
  setReplyText: (text: string) => void;
  setSubmittingReply: (submitting: boolean) => void;

  // Todo panel
  selectedTodoId: string | null;
  setSelectedTodoId: (id: string | null) => void;
  todoPanelVisible: boolean;
  toggleTodoPanel: () => void;

  // Session selection
  selectedSessionName: string | null;
  selectedSessionVersion: string;
  selectSession: (name: string | null) => void;
  selectSessionVersion: (version: string) => void;

  // Navigation helpers
  navigateChanges: (direction: 'up' | 'down', changes: Change[], selectFn?: (id: string) => void) => void;
  navigateThreads: (direction: 'up' | 'down', threadIds: string[]) => void;
  navigateTodos: (direction: 'up' | 'down', todoIds: string[]) => void;
}

const AppContext = createContext<AppContextValue | null>(null);

export function AppProvider({ children }: { children: ReactNode }) {
  // Selection state - uses useTransition for smooth loading
  const [isPending, startTransition] = useTransition();
  const [selection, setSelection] = useState<SelectionState>({
    selectedChangeId: null,
  });

  // UI state - instant updates
  const [ui, setUI] = useState<UIState>({
    focusedPanel: 'changes',
    selectedThreadId: null,
    selectedTodoId: null,
    todoPanelVisible: false,
    selectedSessionName: null,
    selectedSessionVersion: 'live',
    newCommentText: '',
    replyingToThread: false,
    replyText: '',
    submittingReply: false,
  });

  // Selection actions - wrapped in transitions
  const selectChange = useCallback((changeId: string | null) => {
    // Check for unsaved comment text when switching changes
    if (changeId && selection.selectedChangeId && changeId !== selection.selectedChangeId && ui.newCommentText.trim()) {
      const confirmed = window.confirm('You have an unsaved comment. Discard it?');
      if (!confirmed) return;
    }

    // Change ID update is in transition so old diff stays visible during load
    startTransition(() => {
      setSelection(prev => ({
        ...prev,
        selectedChangeId: changeId,
      }));
    });

    // Reset UI state immediately (not in transition)
    setUI(prev => ({
      ...prev,
      selectedThreadId: null,
      newCommentText: '',
      replyingToThread: false,
      replyText: '',
    }));
  }, [selection.selectedChangeId, ui.newCommentText, startTransition]);

  // UI actions - instant, no transitions
  const setFocusedPanel = useCallback((panel: FocusedPanel) => {
    setUI(prev => ({ ...prev, focusedPanel: panel }));
  }, []);

  const cyclePanel = useCallback((reverse: boolean, hasThreads: boolean) => {
    // Order: changes → diff → threads → todos → changes
    setUI(prev => {
      const { focusedPanel, todoPanelVisible } = prev;
      let next: FocusedPanel;

      if (reverse) {
        if (focusedPanel === 'changes') next = todoPanelVisible ? 'todos' : (hasThreads ? 'threads' : 'diff');
        else if (focusedPanel === 'todos') next = hasThreads ? 'threads' : 'diff';
        else if (focusedPanel === 'threads') next = 'diff';
        else next = 'changes'; // diff → changes
      } else {
        if (focusedPanel === 'changes') next = 'diff';
        else if (focusedPanel === 'diff') next = hasThreads ? 'threads' : (todoPanelVisible ? 'todos' : 'changes');
        else if (focusedPanel === 'threads') next = todoPanelVisible ? 'todos' : 'changes';
        else next = 'changes'; // todos → changes
      }

      return { ...prev, focusedPanel: next };
    });
  }, []);

  const setSelectedThreadId = useCallback((id: string | null) => {
    setUI(prev => ({ ...prev, selectedThreadId: id, replyingToThread: false, replyText: '' }));
  }, []);

  const setNewCommentText = useCallback((text: string) => {
    setUI(prev => ({ ...prev, newCommentText: text }));
  }, []);

  const clearNewComment = useCallback(() => {
    setUI(prev => ({ ...prev, newCommentText: '' }));
  }, []);

  const startReply = useCallback((threadId: string) => {
    setUI(prev => ({ ...prev, selectedThreadId: threadId, replyingToThread: true }));
  }, []);

  const cancelReply = useCallback(() => {
    setUI(prev => ({ ...prev, replyingToThread: false, replyText: '' }));
  }, []);

  const setReplyText = useCallback((text: string) => {
    setUI(prev => ({ ...prev, replyText: text }));
  }, []);

  const setSubmittingReply = useCallback((submitting: boolean) => {
    setUI(prev => ({ ...prev, submittingReply: submitting }));
  }, []);

  const setSelectedTodoId = useCallback((id: string | null) => {
    setUI(prev => ({ ...prev, selectedTodoId: id }));
  }, []);

  const toggleTodoPanel = useCallback(() => {
    setUI(prev => ({
      ...prev,
      todoPanelVisible: !prev.todoPanelVisible,
      focusedPanel: !prev.todoPanelVisible ? 'todos' : (prev.focusedPanel === 'todos' ? 'changes' : prev.focusedPanel),
    }));
  }, []);

  const selectSession = useCallback((name: string | null) => {
    setUI(prev => ({
      ...prev,
      selectedSessionName: name,
      selectedSessionVersion: 'live',
    }));
    // Clear change selection when switching sessions
    startTransition(() => {
      setSelection(prev => ({
        ...prev,
        selectedChangeId: null,
      }));
    });
  }, [startTransition]);

  const selectSessionVersion = useCallback((version: string) => {
    setUI(prev => ({ ...prev, selectedSessionVersion: version }));
  }, []);

  // Navigation helpers
  const navigateChanges = useCallback((
    direction: 'up' | 'down',
    changes: Change[],
    selectFn?: (id: string) => void
  ) => {
    if (changes.length === 0) return;

    const currentIdx = selection.selectedChangeId
      ? changes.findIndex((c) => c.change_id === selection.selectedChangeId)
      : -1;

    const nextIdx =
      direction === 'down'
        ? Math.min(currentIdx + 1, changes.length - 1)
        : Math.max(currentIdx - 1, 0);

    if (nextIdx === currentIdx) return;

    const doSelect = selectFn ?? selectChange;
    doSelect(changes[nextIdx].change_id);
  }, [selection.selectedChangeId, selectChange]);

  const navigateThreads = useCallback((direction: 'up' | 'down', threadIds: string[]) => {
    if (threadIds.length === 0) return;

    const currentIdx = ui.selectedThreadId ? threadIds.indexOf(ui.selectedThreadId) : -1;

    let nextIdx: number;
    if (direction === 'down') {
      nextIdx = currentIdx < 0 ? 0 : Math.min(currentIdx + 1, threadIds.length - 1);
    } else {
      nextIdx = currentIdx < 0 ? 0 : Math.max(currentIdx - 1, 0);
    }

    if (nextIdx === currentIdx) return;

    setUI(prev => ({ ...prev, selectedThreadId: threadIds[nextIdx] }));
  }, [ui.selectedThreadId]);

  const navigateTodos = useCallback((direction: 'up' | 'down', todoIds: string[]) => {
    if (todoIds.length === 0) return;

    const currentIdx = ui.selectedTodoId ? todoIds.indexOf(ui.selectedTodoId) : -1;

    let nextIdx: number;
    if (direction === 'down') {
      nextIdx = currentIdx < 0 ? 0 : Math.min(currentIdx + 1, todoIds.length - 1);
    } else {
      nextIdx = currentIdx < 0 ? 0 : Math.max(currentIdx - 1, 0);
    }

    if (nextIdx === currentIdx) return;

    setUI(prev => ({ ...prev, selectedTodoId: todoIds[nextIdx] }));
  }, [ui.selectedTodoId]);

  const value: AppContextValue = {
    // Selection
    selectedChangeId: selection.selectedChangeId,
    selectChange,
    isSelectingChange: isPending,

    // UI
    focusedPanel: ui.focusedPanel,
    selectedThreadId: ui.selectedThreadId,
    newCommentText: ui.newCommentText,
    replyingToThread: ui.replyingToThread,
    replyText: ui.replyText,
    submittingReply: ui.submittingReply,
    setFocusedPanel,
    cyclePanel,
    setSelectedThreadId,
    setNewCommentText,
    clearNewComment,
    startReply,
    cancelReply,
    setReplyText,
    setSubmittingReply,

    // Todo panel
    selectedTodoId: ui.selectedTodoId,
    setSelectedTodoId,
    todoPanelVisible: ui.todoPanelVisible,
    toggleTodoPanel,

    // Session selection
    selectedSessionName: ui.selectedSessionName,
    selectedSessionVersion: ui.selectedSessionVersion,
    selectSession,
    selectSessionVersion,

    // Navigation
    navigateChanges,
    navigateThreads,
    navigateTodos,
  };

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}

export function useAppContext() {
  const context = useContext(AppContext);
  if (!context) {
    throw new Error('useAppContext must be used within AppProvider');
  }
  return context;
}

