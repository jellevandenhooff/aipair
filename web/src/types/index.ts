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
export type { TodoItem } from './TodoItem';
export type { TodoTree } from './TodoTree';

// Import base Change type and extend with API-computed fields
import type { Change as BaseChange } from './Change';

export interface Change extends BaseChange {
  // These fields are computed by the API, not stored in Rust
  merged: boolean;
  open_thread_count: number;
  revision_count: number;
  has_pending_changes: boolean;
  session_name?: string;
}

export interface SessionPush {
  summary: string;
  commit_id: string;
  timestamp: string;
  change_count: number;
}

export interface SessionSummary {
  name: string;
  status: string;
  push_count: number;
  last_push?: string;
  base_bookmark: string;
  open_thread_count: number;
  change_count: number;
  pushes: SessionPush[];
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

