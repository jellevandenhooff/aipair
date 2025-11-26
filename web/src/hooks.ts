import useSWR, { mutate } from 'swr';
import {
  fetchChanges,
  fetchDiff,
  fetchReview,
  createReview,
  replyToThread as apiReplyToThread,
  resolveThread as apiResolveThread,
  reopenThread as apiReopenThread,
  mergeChange as apiMergeChange,
  type Change,
  type DiffResponse,
  type Review,
} from './api';


// Hook for fetching changes list
export function useChanges() {
  return useSWR('changes', () => fetchChanges(), {
    refreshInterval: 3000,
    revalidateOnFocus: false,
  });
}

// Hook for fetching diff for a specific change
export function useDiff(changeId: string | null, commitId?: string, baseCommitId?: string) {
  const key = changeId
    ? ['diff', changeId, commitId ?? 'latest', baseCommitId ?? 'parent']
    : null;

  return useSWR<DiffResponse>(
    key,
    () => fetchDiff(changeId!, commitId, baseCommitId),
    {
      revalidateOnFocus: false,
    }
  );
}

// Hook for fetching review for a specific change
export function useReview(changeId: string | null) {
  const key = changeId ? ['review', changeId] : null;

  const result = useSWR<Review | null>(
    key,
    async () => {
      const review = await fetchReview(changeId!);
      // Auto-create review if it doesn't exist
      if (!review) {
        return createReview(changeId!);
      }
      return review;
    },
    {
      refreshInterval: 3000,
      revalidateOnFocus: false,
    }
  );

  return result;
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

// Re-export Change type for convenience
export type { Change, DiffResponse, Review };
