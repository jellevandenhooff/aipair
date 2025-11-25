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

// Import base Change type and extend with API-computed fields
import type { Change as BaseChange } from './Change';

export interface Change extends BaseChange {
  // These fields are computed by the API, not stored in Rust
  merged: boolean;
  open_thread_count: number;
  revision_count: number;
  has_pending_changes: boolean;
}
