import { useState, useEffect, useCallback, useMemo, useRef, forwardRef, useImperativeHandle } from 'react';
import { VList, VListHandle } from 'virtua';
import { Thread, Diff, Review, addComment, DiffChunk } from '../api';
import { useAppContext } from '../context';
import { replyToThread, resolveThread, reopenThread } from '../hooks';
import { RevisionLabel } from './CommentPanel';
import { mutate } from 'swr';

export interface DiffViewerHandle {
  scrollToThread: (threadId: string) => void;
}

interface DiffViewerProps {
  diff: Diff;
  targetMessage?: string;
  messageDiff?: DiffChunk[];
  review: Review;
  changeId: string;
  description?: string; // Current change description
}

// Special file name for commit message threads
const COMMIT_MESSAGE_FILE = '__commit__';

// Flattened row types for virtualization
type Row =
  | { type: 'file-header'; path: string }
  | { type: 'hunk-header'; header: string }
  | { type: 'line'; file: string; line: ParsedLine }
  | { type: 'thread'; thread: Thread }
  | { type: 'comment-editor'; file: string; lineStart: number; lineEnd: number }
  | { type: 'collapsed'; file: string; lines: ParsedLine[]; id: string }
  | { type: 'commit-header'; changeId: string; commitId: string }
  | { type: 'commit-line'; lineNum: number; content: string; diffTag?: 'equal' | 'delete' | 'insert' };

interface ParsedLine {
  type: 'context' | 'add' | 'delete';
  content: string;
  oldLineNum?: number;
  newLineNum?: number;
}

const CONTEXT_LINES_TO_SHOW = 3;

function parseDiffToRows(raw: string, expandedSections: Set<string>): Row[] {
  const rows: Row[] = [];
  const lines = raw.split('\n');

  let currentFile = '';
  let oldLine = 0;
  let newLine = 0;
  let inHunk = false;

  // First pass: parse all lines
  const allRows: Row[] = [];

  for (const line of lines) {
    if (line.startsWith('diff --git')) {
      inHunk = false;
    } else if (line.startsWith('+++ b/')) {
      currentFile = line.slice(6);
      allRows.push({ type: 'file-header', path: currentFile });
    } else if (line.startsWith('@@')) {
      const match = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      if (match) {
        oldLine = parseInt(match[1], 10);
        newLine = parseInt(match[2], 10);
      }
      allRows.push({ type: 'hunk-header', header: line });
      inHunk = true;
    } else if (inHunk) {
      if (line.startsWith('+')) {
        allRows.push({
          type: 'line',
          file: currentFile,
          line: { type: 'add', content: line.slice(1), newLineNum: newLine++ },
        });
      } else if (line.startsWith('-')) {
        allRows.push({
          type: 'line',
          file: currentFile,
          line: { type: 'delete', content: line.slice(1), oldLineNum: oldLine++ },
        });
      } else if (line.startsWith(' ') || line === '') {
        allRows.push({
          type: 'line',
          file: currentFile,
          line: { type: 'context', content: line.slice(1), oldLineNum: oldLine++, newLineNum: newLine++ },
        });
      }
    }
  }

  // Second pass: collapse context lines
  let i = 0;
  while (i < allRows.length) {
    const row = allRows[i];

    if (row.type !== 'line' || row.line.type !== 'context') {
      rows.push(row);
      i++;
      continue;
    }

    // Found a context line - collect the run of context lines
    const contextLines: ParsedLine[] = [];
    let contextFile = row.file;

    while (i < allRows.length) {
      const r = allRows[i];
      if (r.type === 'line' && r.line.type === 'context' && r.file === contextFile) {
        contextLines.push(r.line);
        i++;
      } else {
        break;
      }
    }

    // If context run is short enough, just show all lines
    if (contextLines.length <= CONTEXT_LINES_TO_SHOW * 2 + 1) {
      for (const line of contextLines) {
        rows.push({ type: 'line', file: contextFile, line });
      }
      continue;
    }

    // Check if this section is expanded
    const collapsedId = `${contextFile}:${contextLines[0].newLineNum}`;
    if (expandedSections.has(collapsedId)) {
      for (const line of contextLines) {
        rows.push({ type: 'line', file: contextFile, line });
      }
      continue;
    }

    // Show first few context lines
    for (let j = 0; j < CONTEXT_LINES_TO_SHOW; j++) {
      rows.push({ type: 'line', file: contextFile, line: contextLines[j] });
    }

    // Add collapsed section
    const hiddenLines = contextLines.slice(CONTEXT_LINES_TO_SHOW, -CONTEXT_LINES_TO_SHOW);
    rows.push({
      type: 'collapsed',
      file: contextFile,
      lines: hiddenLines,
      id: collapsedId,
    });

    // Show last few context lines
    for (let j = contextLines.length - CONTEXT_LINES_TO_SHOW; j < contextLines.length; j++) {
      rows.push({ type: 'line', file: contextFile, line: contextLines[j] });
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
      // Only insert after lines with newLineNum to avoid duplicates on changed lines
      // TODO: support threads on deleted lines (see TODO.md)
      if (row.line.newLineNum !== undefined) {
        const lineThreads = threadsByEndLine.get(key) || [];
        for (const thread of lineThreads) {
          result.push({ type: 'thread', thread });
        }
      }

      // Insert comment editor after the selected line range ends
      // Only insert after lines with newLineNum to avoid duplicates on changed lines
      if (editorKey === key && row.line.newLineNum !== undefined) {
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

export const DiffViewer = forwardRef<DiffViewerHandle, DiffViewerProps>(function DiffViewer(
  { diff, targetMessage, messageDiff, review, changeId, description },
  ref
) {
  // Get UI state from context
  const {
    selectedRevision,
    comparisonBase,
    focusedPanel,
    selectedThreadId,
    replyingToThread,
    replyText,
    submittingReply,
    newCommentText: commentText,
    setNewCommentText: setCommentText,
    clearNewComment,
    setComparisonBase,
    startReply,
    cancelReply,
    setReplyText,
    setSubmittingReply,
  } = useAppContext();
  const focused = focusedPanel === 'diff';

  // Local state
  const [focusedIndex, setFocusedIndex] = useState(0);
  const [editorOpen, setEditorOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [expandedSections, setExpandedSections] = useState<Set<string>>(new Set());

  const listRef = useRef<VListHandle>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const replyTextareaRef = useRef<HTMLTextAreaElement>(null);

  // Parse diff into base rows
  const baseRows = useMemo(() => {
    return parseDiffToRows(diff.raw, expandedSections);
  }, [diff.raw, expandedSections]);

  // Build commit message rows
  const commitRows = useMemo((): Row[] => {
    // Use targetMessage if viewing a specific revision, otherwise use current description
    const messageToShow = targetMessage ?? description;
    if (!messageToShow) return [];

    const rows: Row[] = [
      {
        type: 'commit-header',
        changeId: changeId,
        commitId: '', // Commit ID not directly available from diff
      },
    ];

    // If comparing revisions with a message diff, show diff lines
    if (messageDiff && messageDiff.length > 0) {
      let lineNum = 1;
      for (const chunk of messageDiff) {
        // Split chunk text into lines (preserving empty lines)
        const chunkLines = chunk.text.split('\n');
        // The split creates an extra empty string at the end if text ends with \n
        // We handle this by checking if the last element is empty
        for (let i = 0; i < chunkLines.length; i++) {
          const line = chunkLines[i];
          // Skip the trailing empty string from split (but keep intentional empty lines)
          if (i === chunkLines.length - 1 && line === '' && chunk.text.endsWith('\n')) {
            continue;
          }
          rows.push({
            type: 'commit-line',
            lineNum: lineNum++,
            content: line,
            diffTag: chunk.tag,
          });
        }
      }
    } else {
      // Normal view: show message (targetMessage for specific revision, or current)
      const lines = messageToShow.split('\n');
      lines.forEach((content, idx) => {
        rows.push({ type: 'commit-line', lineNum: idx + 1, content });
      });
    }
    return rows;
  }, [changeId, description, targetMessage, messageDiff]);

  // Build rows with threads, selectedLines derivation, and editor insertion
  const { rows, selectedLines } = useMemo(() => {
    const threads = review.threads;

    // Separate commit message threads from file threads
    const commitThreads = threads.filter((t) => t.file === COMMIT_MESSAGE_FILE);
    const fileThreads = threads.filter((t) => t.file !== COMMIT_MESSAGE_FILE);

    // Insert threads and editor into commit rows
    let commitRowsWithThreads: Row[] = [];
    for (const row of commitRows) {
      commitRowsWithThreads.push(row);
      if (row.type === 'commit-line') {
        // Insert threads that end on this line
        const lineThreads = commitThreads.filter((t) => t.line_end === row.lineNum);
        for (const thread of lineThreads) {
          commitRowsWithThreads.push({ type: 'thread', thread });
        }
      }
    }

    // Combine commit rows + diff rows
    const allBaseRows = [...commitRowsWithThreads, ...baseRows];
    const rowsWithThreads = insertInlineRows(allBaseRows, fileThreads, null);

    // Derive selectedLines from focused row when editor is open
    let selectedLines: { file: string; start: number; end: number } | null = null;
    if (editorOpen) {
      const focusedRow = rowsWithThreads[focusedIndex];
      if (focusedRow?.type === 'line' && focusedRow.line.newLineNum !== undefined) {
        selectedLines = {
          file: focusedRow.file,
          start: focusedRow.line.newLineNum,
          end: focusedRow.line.newLineNum,
        };
      } else if (focusedRow?.type === 'commit-line') {
        selectedLines = {
          file: COMMIT_MESSAGE_FILE,
          start: focusedRow.lineNum,
          end: focusedRow.lineNum,
        };
      }
    }

    // Insert editor row if editing
    let rows: Row[];
    if (selectedLines) {
      if (selectedLines.file === COMMIT_MESSAGE_FILE) {
        // Insert editor after the commit line
        const withEditor: Row[] = [];
        for (const row of commitRowsWithThreads) {
          withEditor.push(row);
          if (row.type === 'commit-line' && row.lineNum === selectedLines.end) {
            withEditor.push({
              type: 'comment-editor',
              file: COMMIT_MESSAGE_FILE,
              lineStart: selectedLines.start,
              lineEnd: selectedLines.end,
            });
          }
        }
        rows = [...withEditor, ...insertInlineRows(baseRows, fileThreads, null)];
      } else {
        rows = [...commitRowsWithThreads, ...insertInlineRows(baseRows, fileThreads, selectedLines)];
      }
    } else {
      rows = rowsWithThreads;
    }

    return { rows, selectedLines };
  }, [baseRows, commitRows, review.threads, editorOpen, focusedIndex]);

  // Build map of threadId -> row index for scrolling
  const threadRowIndices = useMemo(() => {
    const map = new Map<string, number>();
    rows.forEach((row, idx) => {
      if (row.type === 'thread') {
        map.set(row.thread.id, idx);
      }
    });
    return map;
  }, [rows]);

  // Expose scrollToThread via ref
  useImperativeHandle(
    ref,
    () => ({
      scrollToThread: (threadId: string) => {
        const idx = threadRowIndices.get(threadId);
        if (idx !== undefined) {
          listRef.current?.scrollToIndex(idx, { align: 'center' });
        }
      },
    }),
    [threadRowIndices]
  );

  // Find indices of navigable rows (lines, threads, collapsed sections, and commit lines) for keyboard nav
  const navigableIndices = useMemo(() => {
    return rows
      .map((row, idx) =>
        row.type === 'line' || row.type === 'thread' || row.type === 'collapsed' || row.type === 'commit-line'
          ? idx
          : -1
      )
      .filter((idx) => idx !== -1);
  }, [rows]);

  // Track change_id to detect actual changeset switches (not just polling refreshes)
  const prevChangeIdRef = useRef<string | null>(null);
  useEffect(() => {
    const currentChangeId = diff?.change_id ?? null;
    if (prevChangeIdRef.current !== null && prevChangeIdRef.current !== currentChangeId) {
      // Changeset actually switched - reset state
      if (!commentText.trim()) {
        setEditorOpen(false);
        setFocusedIndex(0);
      }
      setExpandedSections(new Set());
    }
    prevChangeIdRef.current = currentChangeId;
  }, [diff?.change_id]);

  // Focus textarea when editor appears
  useEffect(() => {
    if (selectedLines && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [selectedLines]);

  // Focus reply textarea when replying starts
  useEffect(() => {
    if (replyingToThread && focused && replyTextareaRef.current) {
      replyTextareaRef.current.focus();
    }
  }, [replyingToThread, focused]);

  // Scroll to focused line when panel becomes focused
  useEffect(() => {
    if (focused && listRef.current) {
      listRef.current.scrollToIndex(focusedIndex, { align: 'center' });
    }
  }, [focused]);

  const handleLineClick = useCallback(
    (rowIndex: number) => {
      setFocusedIndex(rowIndex);
      setEditorOpen(true);
    },
    []
  );

  const handleExpandSection = useCallback((id: string) => {
    setExpandedSections((prev) => {
      const next = new Set(prev);
      next.add(id);
      return next;
    });
  }, []);

  const handleSubmitComment = useCallback(async () => {
    if (!selectedLines || !commentText.trim() || !changeId) return;

    setSubmitting(true);
    try {
      const result = await addComment(
        changeId,
        selectedLines.file,
        selectedLines.start,
        selectedLines.end,
        commentText.trim()
      );
      // Update SWR cache with new review
      mutate(['review', changeId], result.review, false);
      setEditorOpen(false);
      clearNewComment();
    } catch (e) {
      console.error('Failed to add comment:', e);
    } finally {
      setSubmitting(false);
    }
  }, [selectedLines, commentText, changeId, clearNewComment]);

  const handleSubmitReply = useCallback(async (threadId: string) => {
    if (!replyText.trim() || !changeId) return;
    setSubmittingReply(true);
    try {
      await replyToThread(changeId, threadId, replyText.trim());
      setReplyText('');
      cancelReply();
    } catch (e) {
      console.error('Failed to reply:', e);
    } finally {
      setSubmittingReply(false);
    }
  }, [replyText, changeId, setReplyText, cancelReply, setSubmittingReply]);

  const handleToggleThreadStatus = useCallback(async (threadId: string) => {
    const thread = review.threads.find((t) => t.id === threadId);
    if (!thread) return;

    try {
      if (thread.status === 'open') {
        await resolveThread(changeId, threadId);
      } else {
        await reopenThread(changeId, threadId);
      }
    } catch (e) {
      console.error('Failed to toggle thread status:', e);
    }
  }, [changeId, review.threads]);

  // Keyboard navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Don't handle if typing in textarea
      if (e.target instanceof HTMLTextAreaElement) return;

      // Only handle navigation keys when this panel is focused
      if (!focused) return;

      const currentNavIdx = navigableIndices.indexOf(focusedIndex);

      switch (e.key) {
        case 'j':
        case 'ArrowDown': {
          e.preventDefault();
          const nextIdx = currentNavIdx + 1;
          if (nextIdx < navigableIndices.length) {
            const newFocusedIndex = navigableIndices[nextIdx];
            setFocusedIndex(newFocusedIndex);
            listRef.current?.scrollToIndex(newFocusedIndex, { align: 'center' });
          }
          break;
        }
        case 'k':
        case 'ArrowUp': {
          e.preventDefault();
          const prevIdx = currentNavIdx - 1;
          if (prevIdx >= 0) {
            const newFocusedIndex = navigableIndices[prevIdx];
            setFocusedIndex(newFocusedIndex);
            listRef.current?.scrollToIndex(newFocusedIndex, { align: 'center' });
          }
          break;
        }
        case 'Enter':
        case 'c': {
          const row = rows[focusedIndex];
          // Expand collapsed section
          if (row.type === 'collapsed') {
            e.preventDefault();
            handleExpandSection(row.id);
            break;
          }
          // Create new comment - works on code lines and commit lines
          if (row.type === 'line' && row.line.newLineNum !== undefined) {
            e.preventDefault();
            setEditorOpen(true);
          } else if (row.type === 'commit-line') {
            e.preventDefault();
            setEditorOpen(true);
          }
          break;
        }
        case 'Escape': {
          setEditorOpen(false);
          clearNewComment();
          cancelReply();
          break;
        }
        case 'g': {
          e.preventDefault();
          if (navigableIndices.length > 0) {
            const newIdx = navigableIndices[0];
            setFocusedIndex(newIdx);
            listRef.current?.scrollToIndex(newIdx);
          }
          break;
        }
        case 'G': {
          e.preventDefault();
          if (navigableIndices.length > 0) {
            const newIdx = navigableIndices[navigableIndices.length - 1];
            setFocusedIndex(newIdx);
            listRef.current?.scrollToIndex(newIdx);
          }
          break;
        }
        case 'd': {
          if (!e.ctrlKey) return;
          e.preventDefault();
          const halfPage = 15;
          const nextIdx = Math.min(currentNavIdx + halfPage, navigableIndices.length - 1);
          if (nextIdx >= 0) {
            const newFocusedIndex = navigableIndices[nextIdx];
            setFocusedIndex(newFocusedIndex);
            listRef.current?.scrollToIndex(newFocusedIndex, { align: 'center' });
          }
          break;
        }
        case 'u': {
          if (!e.ctrlKey) return;
          e.preventDefault();
          const halfPage = 15;
          const prevIdx = Math.max(currentNavIdx - halfPage, 0);
          if (navigableIndices.length > 0) {
            const newFocusedIndex = navigableIndices[prevIdx];
            setFocusedIndex(newFocusedIndex);
            listRef.current?.scrollToIndex(newFocusedIndex, { align: 'center' });
          }
          break;
        }
        case 'r': {
          // Reply to selected thread (must be focused on thread row)
          const row = rows[focusedIndex];
          if (row.type === 'thread' && row.thread.status === 'open') {
            e.preventDefault();
            startReply(row.thread.id);
          }
          break;
        }
        case 'x': {
          // Toggle thread status (resolve/reopen)
          const row = rows[focusedIndex];
          if (row.type === 'thread') {
            e.preventDefault();
            handleToggleThreadStatus(row.thread.id);
          }
          break;
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [focusedIndex, navigableIndices, rows, review, focused, startReply, handleToggleThreadStatus, cancelReply, handleExpandSection]);

  // Render function - only called for visible rows
  const renderRow = useCallback(
    (row: Row, idx: number) => {
      if (row.type === 'commit-header') {
        return (
          <div className="bg-amber-50 border-b border-amber-200 px-4 py-2 sticky top-0 z-10">
            <span className="text-amber-700 font-semibold">Commit Message</span>
          </div>
        );
      }

      if (row.type === 'commit-line') {
        const isFocusedLine = idx === focusedIndex;
        const isEditing = isFocusedLine && editorOpen;
        const { diffTag } = row;

        // Background colors based on diff tag
        const getBgClass = () => {
          if (isEditing) return 'bg-blue-200 ring-1 ring-blue-400';
          if (isFocusedLine && focused) {
            if (diffTag === 'delete') return 'bg-red-200';
            if (diffTag === 'insert') return 'bg-green-200';
            return 'bg-amber-100';
          }
          if (isFocusedLine) {
            if (diffTag === 'delete') return 'bg-red-100';
            if (diffTag === 'insert') return 'bg-green-100';
            return 'bg-amber-50';
          }
          if (diffTag === 'delete') return 'bg-red-50';
          if (diffTag === 'insert') return 'bg-green-50';
          if (diffTag === 'equal') return 'bg-yellow-50/50';
          return 'bg-amber-50/50';
        };

        const getTextClass = () => {
          if (diffTag === 'delete') return 'text-red-700';
          if (diffTag === 'insert') return 'text-green-700';
          return 'text-gray-700';
        };

        const getBorderClass = () => {
          if (diffTag === 'delete') return 'border-r-red-200';
          if (diffTag === 'insert') return 'border-r-green-200';
          return 'border-r-amber-200';
        };

        return (
          <div
            onClick={() => {
              if (review) {
                setFocusedIndex(idx);
                setEditorOpen(true);
              }
            }}
            className={`flex border-l-2 ${getBgClass()} border-transparent ${!isFocusedLine ? 'cursor-pointer hover:bg-amber-100' : ''}`}
          >
            <span className="w-12 text-right pr-2 select-none shrink-0 text-amber-400">
              {row.lineNum}
            </span>
            <span className={`w-12 text-right pr-4 select-none border-r ${getBorderClass()} shrink-0`}>
              {diffTag === 'delete' && <span className="text-red-400">-</span>}
              {diffTag === 'insert' && <span className="text-green-400">+</span>}
            </span>
            <span className={`pl-4 whitespace-pre-wrap break-all flex-1 ${getTextClass()}`}>
              {row.content || '\u00A0'}
            </span>
          </div>
        );
      }

      if (row.type === 'file-header') {
        return (
          <div className="bg-gray-100 border-b border-gray-200 px-4 py-2 sticky top-0 z-10">
            <span className="text-blue-600 font-semibold">{row.path}</span>
          </div>
        );
      }

      if (row.type === 'hunk-header') {
        return (
          <div className="bg-gray-50 text-gray-400 px-4 py-1 text-xs">{row.header}</div>
        );
      }

      if (row.type === 'thread') {
        const { thread } = row;
        const isFocusedRow = idx === focusedIndex;
        return (
          <div
            onClick={() => setFocusedIndex(idx)}
            className={`ml-24 mr-4 my-2 bg-white border rounded-lg p-3 font-sans shadow-sm cursor-pointer transition-colors ${
              isFocusedRow && focused
                ? 'border-blue-500 ring-2 ring-blue-200'
                : isFocusedRow
                  ? 'border-blue-300'
                  : 'border-gray-200 hover:border-gray-300'
            }`}
          >
            <div className="text-xs text-gray-400 mb-2 font-mono">
              {thread.file}:{thread.line_start}-{thread.line_end}
              <span className="ml-2 text-gray-300">[{thread.id}]</span>
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

            {/* Actions for focused thread - only when diff panel is focused */}
            {isFocusedRow && focused && (
              <div className="mt-3 pt-3 border-t border-gray-100">
                {thread.status === 'resolved' ? (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleToggleThreadStatus(thread.id);
                    }}
                    className="px-2 py-1 text-xs text-amber-600 hover:text-amber-700 hover:bg-amber-50 rounded"
                  >
                    Reopen
                  </button>
                ) : replyingToThread && selectedThreadId === thread.id ? (
                  // TODO: replyingToThread (bool) + selectedThreadId (string) is redundant.
                  // Could simplify to just replyingToThreadId: string | null in the store.
                  <>
                    <textarea
                      ref={replyTextareaRef}
                      value={replyText}
                      onChange={(e) => setReplyText(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter' && !e.shiftKey) {
                          e.preventDefault();
                          if (replyText.trim()) handleSubmitReply(thread.id);
                        } else if (e.key === 'Escape') {
                          cancelReply();
                        }
                      }}
                      onClick={(e) => e.stopPropagation()}
                      placeholder="Reply... (Enter to send, Esc to cancel)"
                      className="w-full bg-gray-50 border border-gray-200 rounded p-2 text-sm resize-none"
                      rows={2}
                    />
                    <div className="flex justify-between mt-2">
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          cancelReply();
                        }}
                        className="px-2 py-1 text-xs text-gray-500 hover:text-gray-700"
                      >
                        Cancel
                      </button>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          if (replyText.trim()) handleSubmitReply(thread.id);
                        }}
                        disabled={!replyText.trim() || submittingReply}
                        className="px-3 py-1 text-sm bg-blue-600 hover:bg-blue-700 text-white rounded disabled:opacity-50"
                      >
                        {submittingReply ? '...' : 'Send'}
                      </button>
                    </div>
                  </>
                ) : (
                  <div className="flex justify-between items-center">
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        handleToggleThreadStatus(thread.id);
                      }}
                      className="px-2 py-1 text-xs text-green-600 hover:text-green-700 hover:bg-green-100 rounded"
                    >
                      Resolve
                    </button>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        startReply(thread.id);
                      }}
                      className="px-3 py-1 text-sm text-blue-600 hover:text-blue-700 hover:bg-blue-50 rounded"
                    >
                      Reply
                    </button>
                  </div>
                )}
              </div>
            )}
          </div>
        );
      }

      if (row.type === 'comment-editor') {
        return (
          <div className="ml-24 mr-4 my-2 bg-white border border-blue-400 rounded-lg p-3 font-sans shadow-sm ring-2 ring-blue-200">
            <div className="text-xs text-gray-500 mb-2">
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
                  setEditorOpen(false);
                  clearNewComment();
                }
              }}
              placeholder="Add your comment... (Enter to submit, Esc to cancel)"
              className="w-full bg-gray-50 border border-gray-200 rounded p-2 text-sm resize-none"
              rows={3}
            />
            <div className="flex justify-end gap-2 mt-2">
              <button
                onClick={() => {
                  setEditorOpen(false);
                  clearNewComment();
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

      if (row.type === 'collapsed') {
        const isFocusedRow = idx === focusedIndex;
        return (
          <div
            onClick={() => handleExpandSection(row.id)}
            className={`flex border-y text-xs py-1 cursor-pointer ${
              isFocusedRow && focused
                ? 'bg-blue-100 border-blue-300 text-blue-700'
                : isFocusedRow
                  ? 'bg-blue-50 border-gray-200 text-gray-600'
                  : 'bg-gray-50 border-gray-200 text-gray-500 hover:bg-gray-100 hover:text-gray-700'
            }`}
          >
            <span className="w-24 text-center shrink-0">⋯</span>
            <span className="pl-4">
              {row.lines.length} lines hidden — {isFocusedRow && focused ? 'Enter to expand' : 'click to expand'}
            </span>
          </div>
        );
      }

      // line row
      const { line } = row;
      const isFocusedLine = idx === focusedIndex;
      const isEditing = isFocusedLine && editorOpen;
      // Only allow clicking on lines with newLineNum (not pure deletions)
      const canClick = line.newLineNum !== undefined && line.newLineNum > 0;

      return (
        <div
          onClick={() => canClick && handleLineClick(idx)}
          className={`flex border-l-2 ${
            isEditing
              ? 'bg-blue-200 ring-1 ring-blue-400'
              : isFocusedLine && focused
                ? 'bg-blue-100'
                : isFocusedLine
                  ? 'bg-blue-50'
                  : line.type === 'add'
                    ? 'bg-green-50'
                    : line.type === 'delete'
                      ? 'bg-red-50'
                      : ''
          } border-transparent ${
            canClick && !isFocusedLine ? 'cursor-pointer hover:bg-gray-100' : ''
          }`}
        >
          <span className="w-12 text-right pr-2 select-none shrink-0 text-gray-400">
            {line.oldLineNum ?? ''}
          </span>
          <span className="w-12 text-right pr-4 select-none border-r border-gray-200 shrink-0 text-gray-400">
            {line.newLineNum ?? ''}
          </span>
          <span className="pl-4 whitespace-pre-wrap break-all flex-1">
            <span
              className={`${
                line.type === 'add'
                  ? 'text-green-700'
                  : line.type === 'delete'
                    ? 'text-red-700'
                    : ''
              }`}
            >
              {line.type === 'add' && '+'}
              {line.type === 'delete' && '-'}
              {line.type === 'context' && ' '}
              {line.content}
            </span>
          </span>
        </div>
      );
    },
    [
      focusedIndex,
      selectedLines,
      review,
      handleLineClick,
      handleExpandSection,
      focused,
      editorOpen,
      commentText,
      submitting,
      handleSubmitComment,
      selectedThreadId,
      replyingToThread,
      startReply,
      handleSubmitReply,
      cancelReply,
      handleToggleThreadStatus,
      replyText,
      setReplyText,
      submittingReply,
      selectedRevision,
      comparisonBase,
    ]
  );

  if (!diff || rows.length === 0) {
    return <div className="p-8 text-center text-gray-400">No diff content</div>;
  }

  // Determine what we're showing
  const latestRevision = review.revisions[review.revisions.length - 1];
  const showingRevision = selectedRevision ?? latestRevision;
  const isComparingRevisions = comparisonBase !== null;

  return (
    <div ref={containerRef} className="h-full font-mono text-sm flex flex-col" tabIndex={0}>
      {/* Comparison header */}
      {showingRevision && (() => {
        const isPending = showingRevision.is_pending;
        const bgClass = isComparingRevisions ? 'bg-purple-50 border-purple-200' : isPending ? 'bg-blue-50 border-blue-200' : 'bg-gray-50 border-gray-200';
        const textClass = isComparingRevisions ? 'text-purple-700' : isPending ? 'text-blue-700' : 'text-gray-600';
        const fromRevision = comparisonBase ?? 'base';

        return (
          <div className={`${bgClass} border-b px-4 py-2 flex items-center justify-between shrink-0`}>
            <span className={`${textClass} text-sm`}>
              {isComparingRevisions && 'Comparing '}
              <span className="font-semibold"><RevisionLabel revision={fromRevision} /></span>
              {' → '}
              <span className="font-semibold"><RevisionLabel revision={showingRevision} /></span>
              {!isPending && showingRevision.description && (
                <span className="ml-2 text-gray-400">— {showingRevision.description}</span>
              )}
            </span>
            {isComparingRevisions && (
              <button
                onClick={() => setComparisonBase(null)}
                className="text-xs text-purple-600 hover:text-purple-800 underline"
              >
                Show full diff
              </button>
            )}
          </div>
        );
      })()}

      <VList ref={listRef} className="flex-1" data={rows}>
        {renderRow}
      </VList>

      {/* Key bindings hint */}
      <div className="fixed bottom-0 left-0 right-0 bg-gray-800/90 text-gray-300 text-xs px-4 py-2 z-20 backdrop-blur-sm">
        <span className="text-gray-400">Tab</span> switch panel
        <span className="mx-2 text-gray-600">|</span>
        <span className="text-gray-400">j/k</span> navigate
        <span className="mx-2 text-gray-600">|</span>
        <span className="text-gray-400">c</span> comment
        <span className="mx-2 text-gray-600">|</span>
        <span className="text-gray-400">r</span> reply
        <span className="mx-2 text-gray-600">|</span>
        <span className="text-gray-400">x</span> resolve
        <span className="mx-2 text-gray-600">|</span>
        <span className="text-gray-400">g/G</span> top/bottom
      </div>
    </div>
  );
});
