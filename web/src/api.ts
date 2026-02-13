// Import types from generated types
import type { Change, Diff, Review, GraphRow, TodoTree, SessionSummary } from './types';

// Re-export types for consumers
export type { Change, Diff, FileDiff, Review, Thread, Comment, Author, ThreadStatus, GraphRow, NodeLine, PadLine, TodoItem, TodoTree, SessionSummary } from './types';

const API_BASE = '/api';

export interface ChangesData {
  changes: Change[];
  graph: GraphRow[];
  sessions: SessionSummary[];
}

export async function fetchChanges(): Promise<ChangesData> {
  const res = await fetch(`${API_BASE}/changes`);
  if (!res.ok) throw new Error(`Failed to fetch changes: ${res.statusText}`);
  const data = await res.json();
  return { changes: data.changes, graph: data.graph, sessions: data.sessions ?? [] };
}

export interface DiffChunk {
  tag: 'equal' | 'delete' | 'insert';
  text: string;
}

export interface DiffResponse {
  diff: Diff;
  target_message?: string;
  message_diff?: DiffChunk[];
}

export async function fetchDiff(changeId: string, commitId?: string, baseCommitId?: string, session?: string): Promise<DiffResponse> {
  const params = new URLSearchParams();
  if (commitId) params.set('commit', commitId);
  if (baseCommitId) params.set('base', baseCommitId);
  if (session) params.set('session', session);
  const query = params.toString();
  const url = query
    ? `${API_BASE}/changes/${changeId}/diff?${query}`
    : `${API_BASE}/changes/${changeId}/diff`;
  const res = await fetch(url);
  if (!res.ok) throw new Error(`Failed to fetch diff: ${res.statusText}`);
  const data = await res.json();
  return { diff: data.diff, target_message: data.target_message, message_diff: data.message_diff };
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

// Todo API functions

export async function fetchTodos(): Promise<TodoTree> {
  const res = await fetch(`${API_BASE}/todos`);
  if (!res.ok) throw new Error(`Failed to fetch todos: ${res.statusText}`);
  return res.json();
}

export async function createTodo(
  text: string,
  parentId?: string | null,
  afterId?: string | null,
): Promise<TodoTree> {
  const res = await fetch(`${API_BASE}/todos`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      text,
      parent_id: parentId ?? null,
      after_id: afterId ?? null,
    }),
  });
  if (!res.ok) throw new Error(`Failed to create todo: ${res.statusText}`);
  return res.json();
}

export async function updateTodo(
  id: string,
  updates: {
    text?: string;
    checked?: boolean;
    parent_id?: string | null;
    after_id?: string | null;
  },
): Promise<TodoTree> {
  const res = await fetch(`${API_BASE}/todos/${id}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updates),
  });
  if (!res.ok) throw new Error(`Failed to update todo: ${res.statusText}`);
  return res.json();
}

export async function deleteTodo(id: string): Promise<TodoTree> {
  const res = await fetch(`${API_BASE}/todos/${id}`, {
    method: 'DELETE',
  });
  if (!res.ok) throw new Error(`Failed to delete todo: ${res.statusText}`);
  return res.json();
}

// Session API

export interface SessionChangesData {
  changes: Change[];
  graph: GraphRow[];
  base_commit_id: string | null;
  base_current_commit_id: string | null;
}

// version: "live", "latest", or a push index (0 = oldest)
export async function fetchSessionChanges(name: string, version: string = 'live'): Promise<SessionChangesData> {
  const params = new URLSearchParams({ version });
  const res = await fetch(`${API_BASE}/sessions/${name}/changes?${params}`);
  if (!res.ok) throw new Error(`Failed to fetch session changes: ${res.statusText}`);
  const data = await res.json();
  return {
    changes: data.changes,
    graph: data.graph,
    base_commit_id: data.base_commit_id ?? null,
    base_current_commit_id: data.base_current_commit_id ?? null,
  };
}

export async function mergeSession(name: string): Promise<MergeResult> {
  const res = await fetch(`${API_BASE}/sessions/${name}/merge`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({}),
  });
  const data = await res.json();
  if (!res.ok && !data.message) {
    throw new Error(`Failed to merge session: ${res.statusText}`);
  }
  return data;
}

// Timeline API

export interface TimelineEntry {
  timestamp: string;
  type: 'ReviewComment' | 'ReviewReply' | 'ChatMessage' | 'CodeSnapshot';
  // ReviewComment fields
  change_id?: string;
  thread_id?: string;
  file?: string;
  line_start?: number;
  line_end?: number;
  text?: string;
  // ReviewReply fields
  author?: string;
  // ChatMessage fields
  session_id?: string;
  // CodeSnapshot fields
  commit_id?: string;
  description?: string;
}

export async function fetchTimeline(): Promise<TimelineEntry[]> {
  const res = await fetch(`${API_BASE}/timeline`);
  if (!res.ok) throw new Error(`Failed to fetch timeline: ${res.statusText}`);
  return res.json();
}
