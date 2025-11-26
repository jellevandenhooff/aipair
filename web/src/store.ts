import { create } from 'zustand';
import type { Change } from './api';
import type { Revision } from './types';

type FocusedPanel = 'changes' | 'diff' | 'threads';

interface AppState {
  // Selection state
  selectedChangeId: string | null;
  selectedRevision: Revision | null;
  comparisonBase: Revision | null;

  // UI state
  focusedPanel: FocusedPanel;
  selectedThreadId: string | null;

  // New comment state (for diff lines)
  newCommentText: string;

  // Reply state (for existing threads)
  replyingToThread: boolean;
  replyText: string;
  submittingReply: boolean;

  // Actions
  selectChange: (changeId: string | null) => void;
  setFocusedPanel: (panel: FocusedPanel) => void;
  cyclePanel: (reverse: boolean, hasThreads: boolean) => void;
  setSelectedThreadId: (id: string | null) => void;
  setNewCommentText: (text: string) => void;
  clearNewComment: () => void;

  // Reply actions
  startReply: (threadId: string) => void;
  cancelReply: () => void;
  setReplyText: (text: string) => void;
  setSubmittingReply: (submitting: boolean) => void;

  // Revision selection
  selectRevision: (revision: Revision | null) => void;
  setComparisonBase: (revision: Revision | null) => void;

  // Navigation helpers
  navigateChanges: (direction: 'up' | 'down', changes: Change[], selectChange: (id: string) => void) => void;
  navigateThreads: (direction: 'up' | 'down', threadIds: string[]) => void;
}

export const useAppStore = create<AppState>((set, get) => ({
  // Initial state
  selectedChangeId: null,
  selectedRevision: null,
  comparisonBase: null,
  focusedPanel: 'changes',
  selectedThreadId: null,
  newCommentText: '',
  replyingToThread: false,
  replyText: '',
  submittingReply: false,

  // Actions
  selectChange: (changeId) => {
    const { newCommentText, selectedChangeId } = get();

    // Check for unsaved comment text when switching changes
    if (changeId && selectedChangeId && changeId !== selectedChangeId && newCommentText.trim()) {
      const confirmed = window.confirm('You have an unsaved comment. Discard it?');
      if (!confirmed) return;
    }

    set({
      selectedChangeId: changeId,
      selectedRevision: null,
      comparisonBase: null,
      selectedThreadId: null,
      newCommentText: '',
      replyingToThread: false,
      replyText: '',
    });
  },

  setFocusedPanel: (panel) => set({ focusedPanel: panel }),

  cyclePanel: (reverse, hasThreads) => {
    const { focusedPanel } = get();

    if (reverse) {
      if (focusedPanel === 'changes') {
        set({ focusedPanel: hasThreads ? 'threads' : 'diff' });
      } else if (focusedPanel === 'diff') {
        set({ focusedPanel: 'changes' });
      } else {
        set({ focusedPanel: 'diff' });
      }
    } else {
      if (focusedPanel === 'changes') {
        set({ focusedPanel: 'diff' });
      } else if (focusedPanel === 'diff') {
        set({ focusedPanel: hasThreads ? 'threads' : 'changes' });
      } else {
        set({ focusedPanel: 'changes' });
      }
    }
  },

  setSelectedThreadId: (id) => {
    set({ selectedThreadId: id, replyingToThread: false, replyText: '' });
  },

  setNewCommentText: (text) => set({ newCommentText: text }),

  clearNewComment: () => set({ newCommentText: '' }),

  // Reply actions
  startReply: (threadId) => {
    set({ selectedThreadId: threadId, replyingToThread: true });
  },

  cancelReply: () => {
    set({ replyingToThread: false, replyText: '' });
  },

  setReplyText: (text) => set({ replyText: text }),

  setSubmittingReply: (submitting) => set({ submittingReply: submitting }),

  // Revision selection
  selectRevision: (revision) => {
    set({ selectedRevision: revision, comparisonBase: null });
  },

  setComparisonBase: (revision) => {
    set({ comparisonBase: revision });
  },

  // Navigation helpers
  navigateChanges: (direction, changes, selectChangeFn) => {
    const { selectedChangeId } = get();
    if (changes.length === 0) return;

    const currentIdx = selectedChangeId
      ? changes.findIndex((c) => c.change_id === selectedChangeId)
      : -1;

    const nextIdx =
      direction === 'down'
        ? Math.min(currentIdx + 1, changes.length - 1)
        : Math.max(currentIdx - 1, 0);

    if (nextIdx === currentIdx) return;
    selectChangeFn(changes[nextIdx].change_id);
  },

  navigateThreads: (direction, threadIds) => {
    const { selectedThreadId } = get();
    if (threadIds.length === 0) return;

    const currentIdx = selectedThreadId ? threadIds.indexOf(selectedThreadId) : -1;

    let nextIdx: number;
    if (direction === 'down') {
      nextIdx = currentIdx < 0 || currentIdx >= threadIds.length - 1 ? 0 : currentIdx + 1;
    } else {
      nextIdx = currentIdx <= 0 ? threadIds.length - 1 : currentIdx - 1;
    }

    set({ selectedThreadId: threadIds[nextIdx] });
  },
}));
