import useSWR, { mutate } from 'swr';
import {
  fetchChanges,
  fetchDiff,
  fetchReview,
  fetchTopics,
  fetchTodos,
  createReview,
  createTodo as apiCreateTodo,
  updateTodo as apiUpdateTodo,
  deleteTodo as apiDeleteTodo,
  replyToThread as apiReplyToThread,
  resolveThread as apiResolveThread,
  reopenThread as apiReopenThread,
  mergeChange as apiMergeChange,
  mergeSession as apiMergeSession,
  type Change,
  type ChangesData,
  type DiffResponse,
  type Review,
  type TopicsResponse,
  type TodoTree,
  type SessionSummary,
} from './api';

// Hook for fetching changes list with DAG graph (suspense mode - always returns data)
export function useChanges(): ChangesData {
  const { data } = useSWR('changes', () => fetchChanges(), {
    suspense: true,
    refreshInterval: 3000,
    revalidateOnFocus: false,
  });
  return data!;
}

// Hook to check if changes are revalidating (for loading indicator)
export function useChangesIsValidating(): boolean {
  const { isValidating } = useSWR('changes', () => fetchChanges(), {
    refreshInterval: 3000,
    revalidateOnFocus: false,
  });
  return isValidating;
}

// Hook for fetching topics (suspense mode)
export function useTopics(): TopicsResponse {
  const { data } = useSWR('topics', () => fetchTopics(), {
    suspense: true,
    refreshInterval: 3000,
    revalidateOnFocus: false,
  });
  return data!;
}

// Hook for fetching diff (suspense mode - requires changeId)
export function useDiff(changeId: string, commitId?: string, baseCommitId?: string): DiffResponse {
  const key = ['diff', changeId, commitId ?? 'latest', baseCommitId ?? 'parent'];

  const { data } = useSWR<DiffResponse>(
    key,
    () => fetchDiff(changeId, commitId, baseCommitId),
    {
      suspense: true,
      revalidateOnFocus: false,
      // No keepPreviousData - Suspense + useTransition handles showing old UI
      // and ensures atomic update when all data is ready
    }
  );
  return data!;
}

// Hook for fetching review (suspense mode - requires changeId)
export function useReview(changeId: string): Review {
  const key = ['review', changeId];

  const { data } = useSWR<Review>(
    key,
    async () => {
      const review = await fetchReview(changeId);
      // Auto-create review if it doesn't exist
      if (!review) {
        return createReview(changeId);
      }
      return review;
    },
    {
      suspense: true,
      refreshInterval: 3000,
      revalidateOnFocus: false,
      // No keepPreviousData - Suspense + useTransition handles showing old UI
    }
  );
  return data!;
}

// Mutation helpers that update the cache
export async function replyToThread(changeId: string, threadId: string, text: string) {
  const review = await apiReplyToThread(changeId, threadId, text);
  // Update the cache with the new review
  mutate(['review', changeId], review, false);
  return review;
}

export async function resolveThread(changeId: string, threadId: string) {
  const review = await apiResolveThread(changeId, threadId);
  mutate(['review', changeId], review, false);
  return review;
}

export async function reopenThread(changeId: string, threadId: string) {
  const review = await apiReopenThread(changeId, threadId);
  mutate(['review', changeId], review, false);
  return review;
}

export async function mergeChange(changeId: string, force = false) {
  const result = await apiMergeChange(changeId, force);
  if (result.success) {
    // Revalidate the changes list to reflect merged status
    mutate('changes');
  }
  return result;
}

// Hook for fetching todos (suspense mode, polled every 3s)
export function useTodos(): TodoTree {
  const { data } = useSWR('todos', () => fetchTodos(), {
    suspense: true,
    refreshInterval: 3000,
    revalidateOnFocus: false,
  });
  return data!;
}

// Todo mutation helpers
export async function addTodo(text: string, parentId?: string | null, afterId?: string | null) {
  const tree = await apiCreateTodo(text, parentId, afterId);
  mutate('todos', tree, false);
  return tree;
}

export async function updateTodo(id: string, updates: { text?: string; checked?: boolean; parent_id?: string | null; after_id?: string | null }) {
  const tree = await apiUpdateTodo(id, updates);
  mutate('todos', tree, false);
  return tree;
}

export async function removeTodo(id: string) {
  const tree = await apiDeleteTodo(id);
  mutate('todos', tree, false);
  return tree;
}

// Session mutation helpers
export async function mergeSessionAction(name: string) {
  const result = await apiMergeSession(name);
  if (result.success) {
    mutate('changes');
  }
  return result;
}

// Re-export types for convenience
export type { Change, ChangesData, DiffResponse, Review, TopicsResponse, TodoTree, SessionSummary };
