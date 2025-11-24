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

  // Reply state
  replyingToThread: boolean;
  replyText: string;
  submittingReply: boolean;

  // Actions
  fetchChanges: () => Promise<void>;
  selectChange: (change: Change | null) => Promise<void>;
  setFocusedPanel: (panel: FocusedPanel) => void;
  cyclePanel: (reverse: boolean) => void;
  setSelectedThreadId: (id: string | null) => void;
  setReview: (review: Review) => void;
  clearError: () => void;

  // Reply actions
  startReply: (threadId: string) => void;
  cancelReply: () => void;
  setReplyText: (text: string) => void;
  submitReply: (threadId: string) => Promise<void>;
  resolveThread: (threadId: string) => Promise<void>;

  // Navigation helpers
  navigateChanges: (direction: 'up' | 'down') => void;
  navigateThreads: (direction: 'up' | 'down') => void;
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
    if (!change) {
      set({ selectedChange: null, diff: null, review: null, selectedThreadId: null });
      return;
    }

    set({ selectedChange: change, loading: true });
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

  resolveThread: async (threadId) => {
    const { review } = get();
    if (!review) return;

    try {
      const updatedReview = await resolveThread(review.change_id, threadId);
      set({ review: updatedReview });
    } catch (e) {
      console.error('Failed to resolve:', e);
    }
  },

  // Navigation helpers
  navigateChanges: (direction) => {
    const { changes, selectedChange } = get();
    if (changes.length === 0) return;

    const currentIdx = selectedChange
      ? changes.findIndex((c) => c.change_id === selectedChange.change_id)
      : -1;

    let nextIdx: number;
    if (direction === 'down') {
      nextIdx = currentIdx < 0 || currentIdx >= changes.length - 1 ? 0 : currentIdx + 1;
    } else {
      nextIdx = currentIdx <= 0 ? changes.length - 1 : currentIdx - 1;
    }

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
}));
