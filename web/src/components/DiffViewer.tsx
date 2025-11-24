import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { VList, VListHandle } from 'virtua';
import { Diff, Review, Thread, addComment } from '../api';

interface Props {
  diff: Diff;
  review: Review | null;
  onReviewUpdate: (review: Review) => void;
  focused: boolean;
}

// Flattened row types for virtualization
type Row =
  | { type: 'file-header'; path: string }
  | { type: 'hunk-header'; header: string }
  | { type: 'line'; file: string; line: ParsedLine }
  | { type: 'thread'; thread: Thread }
  | { type: 'comment-editor'; file: string; lineStart: number; lineEnd: number };

interface ParsedLine {
  type: 'context' | 'add' | 'delete';
  content: string;
  oldLineNum?: number;
  newLineNum?: number;
}

function parseDiffToRows(raw: string): Row[] {
  const rows: Row[] = [];
  const lines = raw.split('\n');

  let currentFile = '';
  let oldLine = 0;
  let newLine = 0;
  let inHunk = false;

  for (const line of lines) {
    if (line.startsWith('diff --git')) {
      inHunk = false;
    } else if (line.startsWith('+++ b/')) {
      currentFile = line.slice(6);
      rows.push({ type: 'file-header', path: currentFile });
    } else if (line.startsWith('@@')) {
      const match = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      if (match) {
        oldLine = parseInt(match[1], 10);
        newLine = parseInt(match[2], 10);
      }
      rows.push({ type: 'hunk-header', header: line });
      inHunk = true;
    } else if (inHunk) {
      if (line.startsWith('+')) {
        rows.push({
          type: 'line',
          file: currentFile,
          line: { type: 'add', content: line.slice(1), newLineNum: newLine++ },
        });
      } else if (line.startsWith('-')) {
        rows.push({
          type: 'line',
          file: currentFile,
          line: { type: 'delete', content: line.slice(1), oldLineNum: oldLine++ },
        });
      } else if (line.startsWith(' ') || line === '') {
        rows.push({
          type: 'line',
          file: currentFile,
          line: { type: 'context', content: line.slice(1), oldLineNum: oldLine++, newLineNum: newLine++ },
        });
      }
    }
  }

  return rows;
}

// Insert thread and editor rows into the base diff rows
function insertInlineRows(
  baseRows: Row[],
  threads: Thread[],
  selectedLines: { file: string; start: number; end: number } | null
): Row[] {
  const result: Row[] = [];

  // Build a map of file:lineEnd -> threads that end on that line
  const threadsByEndLine = new Map<string, Thread[]>();
  for (const thread of threads) {
    const key = `${thread.file}:${thread.line_end}`;
    const existing = threadsByEndLine.get(key) || [];
    existing.push(thread);
    threadsByEndLine.set(key, existing);
  }

  // Track where to insert the comment editor
  const editorKey = selectedLines ? `${selectedLines.file}:${selectedLines.end}` : null;

  for (const row of baseRows) {
    result.push(row);

    if (row.type === 'line') {
      const lineNum = row.line.newLineNum ?? row.line.oldLineNum ?? 0;
      const key = `${row.file}:${lineNum}`;

      // Insert threads that end on this line
      const lineThreads = threadsByEndLine.get(key) || [];
      for (const thread of lineThreads) {
        result.push({ type: 'thread', thread });
      }

      // Insert comment editor after the selected line range ends
      if (editorKey === key) {
        result.push({
          type: 'comment-editor',
          file: selectedLines!.file,
          lineStart: selectedLines!.start,
          lineEnd: selectedLines!.end,
        });
      }
    }
  }

  return result;
}

export function DiffViewer({ diff, review, onReviewUpdate, focused }: Props) {
  const [selectedLines, setSelectedLines] = useState<{ file: string; start: number; end: number } | null>(null);
  const [commentText, setCommentText] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [focusedIndex, setFocusedIndex] = useState(0);
  const listRef = useRef<VListHandle>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const baseRows = useMemo(() => parseDiffToRows(diff.raw), [diff.raw]);

  // Insert thread rows and editor row
  const rows = useMemo(() => {
    const threads = review?.threads || [];
    return insertInlineRows(baseRows, threads, selectedLines);
  }, [baseRows, review?.threads, selectedLines]);

  // Find indices of line rows only (for keyboard nav)
  const lineIndices = useMemo(() => {
    return rows
      .map((row, idx) => (row.type === 'line' ? idx : -1))
      .filter((idx) => idx !== -1);
  }, [rows]);

  // Build a set of lines with threads for O(1) lookup
  const linesWithThreads = useMemo(() => {
    if (!review) return new Set<string>();
    const set = new Set<string>();
    for (const t of review.threads) {
      for (let i = t.line_start; i <= t.line_end; i++) {
        set.add(`${t.file}:${i}`);
      }
    }
    return set;
  }, [review]);

  // Focus textarea when editor appears
  useEffect(() => {
    if (selectedLines && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [selectedLines]);

  const handleLineClick = useCallback((file: string, lineNum: number) => {
    if (!review) return;

    setSelectedLines((prev) => {
      if (prev && prev.file === file) {
        return {
          file,
          start: Math.min(prev.start, lineNum),
          end: Math.max(prev.end, lineNum),
        };
      }
      return { file, start: lineNum, end: lineNum };
    });
  }, [review]);

  const handleSubmitComment = useCallback(async () => {
    if (!selectedLines || !commentText.trim() || !review) return;

    setSubmitting(true);
    try {
      const result = await addComment(
        review.change_id,
        selectedLines.file,
        selectedLines.start,
        selectedLines.end,
        commentText.trim()
      );
      onReviewUpdate(result.review);
      setSelectedLines(null);
      setCommentText('');
    } catch (e) {
      console.error('Failed to add comment:', e);
    } finally {
      setSubmitting(false);
    }
  }, [selectedLines, commentText, review, onReviewUpdate]);

  // Keyboard navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Don't handle if typing in textarea
      if (e.target instanceof HTMLTextAreaElement) return;

      // Only handle navigation keys when this panel is focused
      if (!focused) return;

      const currentLineIdx = lineIndices.indexOf(focusedIndex);

      switch (e.key) {
        case 'j':
        case 'ArrowDown': {
          e.preventDefault();
          const nextIdx = currentLineIdx + 1;
          if (nextIdx < lineIndices.length) {
            const newFocusedIndex = lineIndices[nextIdx];
            setFocusedIndex(newFocusedIndex);
            listRef.current?.scrollToIndex(newFocusedIndex, { align: 'center' });
          }
          break;
        }
        case 'k':
        case 'ArrowUp': {
          e.preventDefault();
          const prevIdx = currentLineIdx - 1;
          if (prevIdx >= 0) {
            const newFocusedIndex = lineIndices[prevIdx];
            setFocusedIndex(newFocusedIndex);
            listRef.current?.scrollToIndex(newFocusedIndex, { align: 'center' });
          }
          break;
        }
        case 'Enter':
        case 'c': {
          if (!review) return;
          e.preventDefault();
          const row = rows[focusedIndex];
          if (row.type === 'line') {
            const lineNum = row.line.newLineNum ?? row.line.oldLineNum ?? 0;
            if (lineNum > 0) {
              setSelectedLines({ file: row.file, start: lineNum, end: lineNum });
            }
          }
          break;
        }
        case 'Escape': {
          setSelectedLines(null);
          setCommentText('');
          break;
        }
        case 'g': {
          e.preventDefault();
          if (lineIndices.length > 0) {
            setFocusedIndex(lineIndices[0]);
            listRef.current?.scrollToIndex(lineIndices[0]);
          }
          break;
        }
        case 'G': {
          e.preventDefault();
          if (lineIndices.length > 0) {
            const lastIdx = lineIndices[lineIndices.length - 1];
            setFocusedIndex(lastIdx);
            listRef.current?.scrollToIndex(lastIdx);
          }
          break;
        }
        case 'd': {
          if (!e.ctrlKey) return;
          e.preventDefault();
          const halfPage = 15;
          const nextIdx = Math.min(currentLineIdx + halfPage, lineIndices.length - 1);
          if (nextIdx >= 0) {
            const newFocusedIndex = lineIndices[nextIdx];
            setFocusedIndex(newFocusedIndex);
            listRef.current?.scrollToIndex(newFocusedIndex, { align: 'center' });
          }
          break;
        }
        case 'u': {
          if (!e.ctrlKey) return;
          e.preventDefault();
          const halfPage = 15;
          const prevIdx = Math.max(currentLineIdx - halfPage, 0);
          if (lineIndices.length > 0) {
            const newFocusedIndex = lineIndices[prevIdx];
            setFocusedIndex(newFocusedIndex);
            listRef.current?.scrollToIndex(newFocusedIndex, { align: 'center' });
          }
          break;
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [focusedIndex, lineIndices, rows, review, focused]);

  // Render function - only called for visible rows
  const renderRow = useCallback((row: Row, idx: number) => {
    if (row.type === 'file-header') {
      return (
        <div className="bg-gray-100 border-b border-gray-200 px-4 py-2 sticky top-0 z-10">
          <span className="text-blue-600 font-semibold">{row.path}</span>
        </div>
      );
    }

    if (row.type === 'hunk-header') {
      return (
        <div className="bg-gray-50 text-gray-400 px-4 py-1 text-xs">
          {row.header}
        </div>
      );
    }

    if (row.type === 'thread') {
      const { thread } = row;
      return (
        <div className="ml-24 mr-4 my-2 bg-amber-50 border border-amber-200 rounded-lg p-3 font-sans">
          <div className="text-xs text-amber-600 mb-2">
            {thread.file}:{thread.line_start}-{thread.line_end}
            <span className={`ml-2 px-1.5 py-0.5 rounded text-xs ${
              thread.status === 'open' ? 'bg-amber-100 text-amber-700' : 'bg-green-100 text-green-700'
            }`}>
              {thread.status}
            </span>
          </div>
          <div className="space-y-2">
            {thread.comments.map((comment, cidx) => (
              <div
                key={cidx}
                className={`text-sm ${
                  comment.author === 'claude'
                    ? 'bg-purple-50 border-l-2 border-purple-400 pl-2 py-1'
                    : ''
                }`}
              >
                <span
                  className={`text-xs font-semibold ${
                    comment.author === 'claude' ? 'text-purple-600' : 'text-blue-600'
                  }`}
                >
                  {comment.author}:
                </span>{' '}
                <span className="text-gray-700">{comment.text}</span>
              </div>
            ))}
          </div>
        </div>
      );
    }

    if (row.type === 'comment-editor') {
      return (
        <div className="ml-24 mr-4 my-2 bg-blue-50 border border-blue-200 rounded-lg p-3 font-sans">
          <div className="text-xs text-blue-600 mb-2">
            New comment on lines {row.lineStart}-{row.lineEnd}
          </div>
          <textarea
            ref={textareaRef}
            value={commentText}
            onChange={(e) => setCommentText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                if (commentText.trim()) handleSubmitComment();
              } else if (e.key === 'Escape') {
                setSelectedLines(null);
                setCommentText('');
              }
            }}
            placeholder="Add your comment... (Enter to submit, Esc to cancel)"
            className="w-full bg-white border border-blue-200 rounded p-2 text-sm resize-none"
            rows={3}
          />
          <div className="flex justify-end gap-2 mt-2">
            <button
              onClick={() => {
                setSelectedLines(null);
                setCommentText('');
              }}
              className="px-3 py-1 text-sm text-gray-500 hover:text-gray-700"
            >
              Cancel
            </button>
            <button
              onClick={handleSubmitComment}
              disabled={!commentText.trim() || submitting}
              className="px-3 py-1 text-sm bg-blue-600 hover:bg-blue-700 text-white rounded disabled:opacity-50"
            >
              {submitting ? 'Adding...' : 'Add Comment'}
            </button>
          </div>
        </div>
      );
    }

    // line row
    const { file, line } = row;
    const lineNum = line.newLineNum ?? line.oldLineNum ?? 0;
    const hasThread = linesWithThreads.has(`${file}:${lineNum}`);
    const selected = selectedLines && selectedLines.file === file &&
                     lineNum >= selectedLines.start && lineNum <= selectedLines.end;
    const isFocusedLine = idx === focusedIndex;

    return (
      <div
        onClick={() => lineNum > 0 && handleLineClick(file, lineNum)}
        className={`flex border-l-2 ${
          isFocusedLine && focused
            ? 'bg-blue-100'
            : isFocusedLine
              ? 'bg-blue-50'
              : line.type === 'add'
                ? 'bg-green-50'
                : line.type === 'delete'
                  ? 'bg-red-50'
                  : ''
        } ${
          selected ? 'bg-blue-200 ring-1 ring-blue-400' : ''
        } ${
          hasThread ? 'border-amber-400' : 'border-transparent'
        } ${
          review && !isFocusedLine ? 'cursor-pointer hover:bg-gray-100' : ''
        }`}
      >
        <span className="w-12 text-right pr-2 select-none shrink-0 text-gray-400">
          {line.oldLineNum ?? ''}
        </span>
        <span className="w-12 text-right pr-4 select-none border-r border-gray-200 shrink-0 text-gray-400">
          {line.newLineNum ?? ''}
        </span>
        <span className="pl-4 whitespace-pre flex-1">
          <span className={`${
            line.type === 'add'
              ? 'text-green-700'
              : line.type === 'delete'
                ? 'text-red-700'
                : ''
          }`}>
            {line.type === 'add' && '+'}
            {line.type === 'delete' && '-'}
            {line.type === 'context' && ' '}
            {line.content}
          </span>
        </span>
      </div>
    );
  }, [focusedIndex, selectedLines, linesWithThreads, review, handleLineClick, focused, commentText, submitting, handleSubmitComment]);

  if (rows.length === 0) {
    return (
      <div className="p-8 text-center text-gray-400">
        No diff content
      </div>
    );
  }

  return (
    <div ref={containerRef} className="h-full font-mono text-sm" tabIndex={0}>
      <VList ref={listRef} className="h-full" data={rows}>
        {renderRow}
      </VList>

      {/* Key bindings hint */}
      <div className="fixed bottom-4 left-4 text-xs text-gray-400 z-20">
        Tab: switch panel | j/k: navigate | ctrl-d/u: page | c: comment | g/G: top/bottom
      </div>
    </div>
  );
}
