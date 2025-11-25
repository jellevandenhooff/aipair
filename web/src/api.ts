// Import types from generated types
import type { Change, Diff, Review } from './types';

// Re-export types for consumers
export type { Change, Diff, FileDiff, Review, Thread, Comment, Author, ThreadStatus } from './types';

const API_BASE = '/api';

export async function fetchChanges(): Promise<Change[]> {
  const res = await fetch(`${API_BASE}/changes`);
  if (!res.ok) throw new Error(`Failed to fetch changes: ${res.statusText}`);
  const data = await res.json();
  return data.changes;
}

export async function fetchDiff(changeId: string, commitId?: string): Promise<Diff> {
  const url = commitId
    ? `${API_BASE}/changes/${changeId}/diff?commit=${encodeURIComponent(commitId)}`
    : `${API_BASE}/changes/${changeId}/diff`;
  const res = await fetch(url);
  if (!res.ok) throw new Error(`Failed to fetch diff: ${res.statusText}`);
  const data = await res.json();
  return data.diff;
}

export async function fetchReview(changeId: string): Promise<Review | null> {
  const res = await fetch(`${API_BASE}/changes/${changeId}/review`);
  if (!res.ok) throw new Error(`Failed to fetch review: ${res.statusText}`);
  const data = await res.json();
  return data.review;
}

export async function createReview(changeId: string, base?: string): Promise<Review> {
  const res = await fetch(`${API_BASE}/changes/${changeId}/review`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ base }),
  });
  if (!res.ok) throw new Error(`Failed to create review: ${res.statusText}`);
  const data = await res.json();
  return data.review;
}

export async function addComment(
  changeId: string,
  file: string,
  lineStart: number,
  lineEnd: number,
  text: string
): Promise<{ review: Review; thread_id: string }> {
  const res = await fetch(`${API_BASE}/changes/${changeId}/comments`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      file,
      line_start: lineStart,
      line_end: lineEnd,
      text,
    }),
  });
  if (!res.ok) throw new Error(`Failed to add comment: ${res.statusText}`);
  return res.json();
}

export async function replyToThread(
  changeId: string,
  threadId: string,
  text: string
): Promise<Review> {
  const res = await fetch(`${API_BASE}/changes/${changeId}/threads/${threadId}/reply`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ text }),
  });
  if (!res.ok) throw new Error(`Failed to reply: ${res.statusText}`);
  const data = await res.json();
  return data.review;
}

export async function resolveThread(
  changeId: string,
  threadId: string
): Promise<Review> {
  const res = await fetch(`${API_BASE}/changes/${changeId}/threads/${threadId}/resolve`, {
    method: 'POST',
  });
  if (!res.ok) throw new Error(`Failed to resolve: ${res.statusText}`);
  const data = await res.json();
  return data.review;
}

export async function reopenThread(
  changeId: string,
  threadId: string
): Promise<Review> {
  const res = await fetch(`${API_BASE}/changes/${changeId}/threads/${threadId}/reopen`, {
    method: 'POST',
  });
  if (!res.ok) throw new Error(`Failed to reopen: ${res.statusText}`);
  const data = await res.json();
  return data.review;
}

export interface MergeResult {
  success: boolean;
  message: string;
}

export async function mergeChange(changeId: string, force = false): Promise<MergeResult> {
  const res = await fetch(`${API_BASE}/changes/${changeId}/merge`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ force }),
  });
  const data = await res.json();
  if (!res.ok && !data.message) {
    throw new Error(`Failed to merge: ${res.statusText}`);
  }
  return data;
}
