import { createContext, useContext, useState, useCallback, useTransition, ReactNode } from 'react';
import { flushSync } from 'react-dom';
import type { Change } from './api';
import type { Revision } from './types';

type FocusedPanel = 'changes' | 'revisions' | 'diff' | 'threads';

// Selection state - triggers data fetching, needs transitions
interface SelectionState {
  selectedChangeId: string | null;
  selectedRevision: Revision | null;
  comparisonBase: Revision | null;
}

// UI state - instant updates, no transitions needed
interface UIState {
  focusedPanel: FocusedPanel;
  selectedThreadId: string | null;
  newCommentText: string;
  replyingToThread: boolean;
  replyText: string;
  submittingReply: boolean;
}

interface AppContextValue {
  // Selection state + actions
  selectedChangeId: string | null;
  selectedRevision: Revision | null;
  comparisonBase: Revision | null;
  selectChange: (changeId: string | null) => void;
  selectRevision: (revision: Revision | null) => void;
  setComparisonBase: (revision: Revision | null) => void;
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

  // Navigation helpers
  navigateChanges: (direction: 'up' | 'down', changes: Change[], selectFn?: (id: string) => void) => void;
  navigateThreads: (direction: 'up' | 'down', threadIds: string[]) => void;
}

const AppContext = createContext<AppContextValue | null>(null);

export function AppProvider({ children }: { children: ReactNode }) {
  // Selection state - uses useTransition for smooth loading
  const [isPending, startTransition] = useTransition();
  const [selection, setSelection] = useState<SelectionState>({
    selectedChangeId: null,
    selectedRevision: null,
    comparisonBase: null,
  });

  // UI state - instant updates
  const [ui, setUI] = useState<UIState>({
    focusedPanel: 'changes',
    selectedThreadId: null,
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

    // Reset revision immediately with flushSync to prevent flash when new review data arrives
    // (old revision number might match a revision in the new change)
    flushSync(() => {
      setSelection(prev => ({
        ...prev,
        selectedRevision: null,
        comparisonBase: null,
      }));
    });

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

  const selectRevision = useCallback((revision: Revision | null) => {
    startTransition(() => {
      setSelection(prev => ({
        ...prev,
        selectedRevision: revision,
        comparisonBase: null,
      }));
    });
  }, [startTransition]);

  const setComparisonBase = useCallback((revision: Revision | null) => {
    setSelection(prev => ({
      ...prev,
      comparisonBase: revision,
    }));
  }, []);

  // UI actions - instant, no transitions
  const setFocusedPanel = useCallback((panel: FocusedPanel) => {
    setUI(prev => ({ ...prev, focusedPanel: panel }));
  }, []);

  const cyclePanel = useCallback((reverse: boolean, hasThreads: boolean) => {
    // Order: changes → diff → revisions → threads → changes
    setUI(prev => {
      const { focusedPanel } = prev;
      let next: FocusedPanel;

      if (reverse) {
        // Reverse: changes → threads → revisions → diff → changes
        if (focusedPanel === 'changes') next = hasThreads ? 'threads' : 'revisions';
        else if (focusedPanel === 'threads') next = 'revisions';
        else if (focusedPanel === 'revisions') next = 'diff';
        else next = 'changes'; // diff → changes
      } else {
        // Forward: changes → diff → revisions → threads → changes
        if (focusedPanel === 'changes') next = 'diff';
        else if (focusedPanel === 'diff') next = 'revisions';
        else if (focusedPanel === 'revisions') next = hasThreads ? 'threads' : 'changes';
        else next = 'changes'; // threads → changes
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
      // If no selection, start at first; otherwise move down (clamped)
      nextIdx = currentIdx < 0 ? 0 : Math.min(currentIdx + 1, threadIds.length - 1);
    } else {
      // If no selection, start at first; otherwise move up (clamped)
      nextIdx = currentIdx < 0 ? 0 : Math.max(currentIdx - 1, 0);
    }

    if (nextIdx === currentIdx) return;

    setUI(prev => ({ ...prev, selectedThreadId: threadIds[nextIdx] }));
  }, [ui.selectedThreadId]);

  const value: AppContextValue = {
    // Selection
    selectedChangeId: selection.selectedChangeId,
    selectedRevision: selection.selectedRevision,
    comparisonBase: selection.comparisonBase,
    selectChange,
    selectRevision,
    setComparisonBase,
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

    // Navigation
    navigateChanges,
    navigateThreads,
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

// Convenience hook for just selection state (common case)
export function useSelection() {
  const ctx = useAppContext();
  return {
    selectedChangeId: ctx.selectedChangeId,
    selectedRevision: ctx.selectedRevision,
    comparisonBase: ctx.comparisonBase,
    selectChange: ctx.selectChange,
    selectRevision: ctx.selectRevision,
    setComparisonBase: ctx.setComparisonBase,
    isSelectingChange: ctx.isSelectingChange,
  };
}
