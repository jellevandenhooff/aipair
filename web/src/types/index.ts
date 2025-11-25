// Auto-generated types from Rust - run `just gen-types` to update
// For now, these are manually maintained until we run type generation

export interface Change {
  change_id: string;
  commit_id: string;
  description: string;
  author: string;
  timestamp: string;
  empty: boolean;
  merged: boolean;
}

export interface FileDiff {
  path: string;
  status: 'added' | 'modified' | 'deleted';
}

export interface Diff {
  change_id: string;
  base: string;
  files: FileDiff[];
  raw: string;
}

export type Author = 'user' | 'claude';

export type ThreadStatus = 'open' | 'resolved';

export interface Comment {
  author: Author;
  text: string;
  timestamp: string;
}

export interface Thread {
  id: string;
  file: string;
  line_start: number;
  line_end: number;
  status: ThreadStatus;
  comments: Comment[];
}

export interface Review {
  change_id: string;
  base: string;
  created_at: string;
  threads: Thread[];
}
