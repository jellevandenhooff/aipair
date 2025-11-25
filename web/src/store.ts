import { create } from 'zustand';
import {
  Change,
  Diff,
  Review,
  fetchChanges as apiFetchChanges,
  fetchDiff,
  fetchReview,
  createReview,
  replyToThread,
  resolveThread,
  reopenThread,
  mergeChange as apiMergeChange,
} from './api';

type FocusedPanel = 'changes' | 'diff' | 'threads';

interface AppState {
  // Data
  changes: Change[];
  selectedChange: Change | null;
  diff: Diff | null;
  review: Review | null;

  // Loading/error
  loading: boolean;
  error: string | null;

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
  fetchChanges: () => Promise<void>;
  selectChange: (change: Change | null) => Promise<void>;
  refreshData: () => Promise<void>;
  setFocusedPanel: (panel: FocusedPanel) => void;
  cyclePanel: (reverse: boolean) => void;
  setSelectedThreadId: (id: string | null) => void;
  setReview: (review: Review) => void;
  clearError: () => void;
  setNewCommentText: (text: string) => void;
  clearNewComment: () => void;

  // Reply actions
  startReply: (threadId: string) => void;
  cancelReply: () => void;
  setReplyText: (text: string) => void;
  submitReply: (threadId: string) => Promise<void>;
  toggleThreadStatus: (threadId: string) => Promise<void>;

  // Navigation helpers
  navigateChanges: (direction: 'up' | 'down') => void;
  navigateThreads: (direction: 'up' | 'down') => void;

  // Merge actions
  mergeChange: (changeId: string, force?: boolean) => Promise<{ success: boolean; message: string }>;
}

export const useAppStore = create<AppState>((set, get) => ({
  // Initial state
  changes: [],
  selectedChange: null,
  diff: null,
  review: null,
  loading: true,
  error: null,
  focusedPanel: 'changes',
  selectedThreadId: null,
  newCommentText: '',
  replyingToThread: false,
  replyText: '',
  submittingReply: false,

  // Actions
  // NOTE: Async actions have potential race conditions (e.g., rapid clicks could
  // cause responses to arrive out of order). Common solutions: AbortController to
  // cancel stale requests, or tracking request IDs to ignore outdated responses.
  // For now we accept this limitation since it's unlikely in normal use.
  fetchChanges: async () => {
    set({ loading: true, error: null });
    try {
      const changes = await apiFetchChanges();
      set({ changes, loading: false });
    } catch (e) {
      set({ error: (e as Error).message, loading: false });
    }
  },

  selectChange: async (change) => {
    const { newCommentText, selectedChange } = get();

    // Check for unsaved comment text when switching changes
    if (change && selectedChange && change.change_id !== selectedChange.change_id && newCommentText.trim()) {
      const confirmed = window.confirm('You have an unsaved comment. Discard it?');
      if (!confirmed) return;
    }

    if (!change) {
      set({ selectedChange: null, diff: null, review: null, selectedThreadId: null, newCommentText: '' });
      return;
    }

    set({ selectedChange: change, loading: true, newCommentText: '' });
    try {
      const [diff, review] = await Promise.all([
        fetchDiff(change.change_id),
        fetchReview(change.change_id),
      ]);

      // Auto-create review if it doesn't exist
      if (!review) {
        const newReview = await createReview(change.change_id);
        set({ diff, review: newReview, loading: false, selectedThreadId: null });
      } else {
        set({ diff, review, loading: false, selectedThreadId: null });
      }
    } catch (e) {
      set({ error: (e as Error).message, loading: false });
    }
  },

  refreshData: async () => {
    const { selectedChange } = get();

    try {
      // Always refresh changes list
      const changes = await apiFetchChanges();
      set({ changes });

      // If a change is selected, refresh its diff and review
      if (selectedChange) {
        // Check if the selected change still exists
        const stillExists = changes.some((c) => c.change_id === selectedChange.change_id);
        if (!stillExists) {
          set({ selectedChange: null, diff: null, review: null, selectedThreadId: null });
          return;
        }

        const [diff, review] = await Promise.all([
          fetchDiff(selectedChange.change_id),
          fetchReview(selectedChange.change_id),
        ]);
        set({ diff, review });
      }
    } catch (e) {
      // Silently ignore refresh errors to avoid spamming the user
      console.error('Refresh failed:', e);
    }
  },

  setFocusedPanel: (panel) => set({ focusedPanel: panel }),

  cyclePanel: (reverse) => {
    const { focusedPanel, review } = get();
    const hasThreads = (review?.threads.length ?? 0) > 0;

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

  setReview: (review) => set({ review }),

  clearError: () => set({ error: null }),

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

  submitReply: async (threadId) => {
    const { replyText, review } = get();
    if (!replyText.trim() || !review) return;

    set({ submittingReply: true });
    try {
      const updatedReview = await replyToThread(review.change_id, threadId, replyText.trim());
      set({
        review: updatedReview,
        replyText: '',
        replyingToThread: false,
        submittingReply: false,
      });
    } catch (e) {
      console.error('Failed to reply:', e);
      set({ submittingReply: false });
    }
  },

  toggleThreadStatus: async (threadId) => {
    const { review } = get();
    if (!review) return;

    const thread = review.threads.find((t) => t.id === threadId);
    if (!thread) return;

    try {
      const updatedReview =
        thread.status === 'open'
          ? await resolveThread(review.change_id, threadId)
          : await reopenThread(review.change_id, threadId);
      set({ review: updatedReview });
    } catch (e) {
      console.error('Failed to toggle thread status:', e);
    }
  },

  // Navigation helpers
  navigateChanges: (direction) => {
    const { changes, selectedChange } = get();
    if (changes.length === 0) return;

    const currentIdx = selectedChange
      ? changes.findIndex((c) => c.change_id === selectedChange.change_id)
      : -1;

    const nextIdx =
      direction === 'down'
        ? Math.min(currentIdx + 1, changes.length - 1)
        : Math.max(currentIdx - 1, 0);

    if (nextIdx === currentIdx) return;
    get().selectChange(changes[nextIdx]);
  },

  navigateThreads: (direction) => {
    const { review, selectedThreadId } = get();
    const threadIds = review?.threads.map((t) => t.id) || [];
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

  // Merge actions
  mergeChange: async (changeId, force = false) => {
    const result = await apiMergeChange(changeId, force);
    if (result.success) {
      // Refresh to get updated merged status
      await get().refreshData();
    }
    return result;
  },
}));
