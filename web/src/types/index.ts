// Re-export generated types from ts-rs (run `cargo test` to regenerate)
// Only extend types here when the API adds fields not in the Rust structs

export type { Author } from './Author';
export type { Comment } from './Comment';
export type { Diff } from './Diff';
export type { FileDiff } from './FileDiff';
export type { FileStatus } from './FileStatus';
export type { Review } from './Review';
export type { Revision } from './Revision';
export type { Thread } from './Thread';
export type { ThreadStatus } from './ThreadStatus';
export type { TopicStatus } from './TopicStatus';

// Import base Change type and extend with API-computed fields
import type { Change as BaseChange } from './Change';

export interface Change extends BaseChange {
  // These fields are computed by the API, not stored in Rust
  merged: boolean;
  open_thread_count: number;
  revision_count: number;
  has_pending_changes: boolean;
  topic_id?: string;
}

// DAG graph types (from sapling-renderdag via API)
export type NodeLine = 'Blank' | 'Ancestor' | 'Parent' | 'Node';
export type PadLine = 'Blank' | 'Ancestor' | 'Parent';

export interface GraphRow {
  node: string;       // change_id
  glyph: string;
  merge: boolean;
  node_line: NodeLine[];
  link_line: number[] | null;  // LinkLine bits as u16
  term_line: boolean[] | null;
  pad_lines: PadLine[];
}

// API response types for topics
export interface TopicChangeInfo {
  change_id: string;
  description: string;
  open_thread_count: number;
}

export interface Topic {
  id: string;
  name: string;
  status: 'active' | 'finished';
  change_count: number;
  changes: TopicChangeInfo[];
  notes?: string;
  created_at: string;
}

export interface TopicsResponse {
  topics: Topic[];
}
