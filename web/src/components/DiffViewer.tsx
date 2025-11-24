import { useState } from 'react';
import { Diff, Review, addComment } from '../api';

interface Props {
  diff: Diff;
  review: Review | null;
  onReviewUpdate: (review: Review) => void;
}

interface ParsedHunk {
  header: string;
  lines: ParsedLine[];
}

interface ParsedLine {
  type: 'context' | 'add' | 'delete' | 'header';
  content: string;
  oldLineNum?: number;
  newLineNum?: number;
}

interface ParsedFile {
  path: string;
  hunks: ParsedHunk[];
}

function parseDiff(raw: string): ParsedFile[] {
  const files: ParsedFile[] = [];
  const lines = raw.split('\n');

  let currentFile: ParsedFile | null = null;
  let currentHunk: ParsedHunk | null = null;
  let oldLine = 0;
  let newLine = 0;

  for (const line of lines) {
    if (line.startsWith('diff --git')) {
      if (currentFile) files.push(currentFile);
      currentFile = { path: '', hunks: [] };
      currentHunk = null;
    } else if (line.startsWith('+++ b/')) {
      if (currentFile) currentFile.path = line.slice(6);
    } else if (line.startsWith('@@')) {
      if (currentFile) {
        const match = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
        if (match) {
          oldLine = parseInt(match[1], 10);
          newLine = parseInt(match[2], 10);
        }
        currentHunk = { header: line, lines: [] };
        currentFile.hunks.push(currentHunk);
      }
    } else if (currentHunk) {
      if (line.startsWith('+')) {
        currentHunk.lines.push({
          type: 'add',
          content: line.slice(1),
          newLineNum: newLine++,
        });
      } else if (line.startsWith('-')) {
        currentHunk.lines.push({
          type: 'delete',
          content: line.slice(1),
          oldLineNum: oldLine++,
        });
      } else if (line.startsWith(' ') || line === '') {
        currentHunk.lines.push({
          type: 'context',
          content: line.slice(1),
          oldLineNum: oldLine++,
          newLineNum: newLine++,
        });
      }
    }
  }

  if (currentFile) files.push(currentFile);
  return files;
}

export function DiffViewer({ diff, review, onReviewUpdate }: Props) {
  const [selectedLines, setSelectedLines] = useState<{ file: string; start: number; end: number } | null>(null);
  const [commentText, setCommentText] = useState('');
  const [submitting, setSubmitting] = useState(false);

  const files = parseDiff(diff.raw);

  const handleLineClick = (file: string, lineNum: number) => {
    if (!review) return;

    if (selectedLines && selectedLines.file === file) {
      // Extend selection
      setSelectedLines({
        file,
        start: Math.min(selectedLines.start, lineNum),
        end: Math.max(selectedLines.end, lineNum),
      });
    } else {
      setSelectedLines({ file, start: lineNum, end: lineNum });
    }
  };

  const handleSubmitComment = async () => {
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
  };

  const isLineSelected = (file: string, lineNum: number) => {
    if (!selectedLines || selectedLines.file !== file) return false;
    return lineNum >= selectedLines.start && lineNum <= selectedLines.end;
  };

  const getThreadsForLine = (file: string, lineNum: number) => {
    if (!review) return [];
    return review.threads.filter(
      (t) => t.file === file && lineNum >= t.line_start && lineNum <= t.line_end
    );
  };

  if (files.length === 0) {
    return (
      <div className="p-8 text-center text-gray-500">
        No diff content
      </div>
    );
  }

  return (
    <div className="font-mono text-sm">
      {files.map((file) => (
        <div key={file.path} className="mb-8">
          <div className="bg-gray-800 border-b border-gray-700 px-4 py-2 sticky top-0">
            <span className="text-blue-400">{file.path}</span>
          </div>

          {file.hunks.map((hunk, hunkIdx) => (
            <div key={hunkIdx}>
              <div className="bg-gray-800/50 text-gray-500 px-4 py-1 text-xs">
                {hunk.header}
              </div>
              <table className="w-full">
                <tbody>
                  {hunk.lines.map((line, lineIdx) => {
                    const lineNum = line.newLineNum ?? line.oldLineNum ?? 0;
                    const threads = getThreadsForLine(file.path, lineNum);
                    const selected = isLineSelected(file.path, lineNum);

                    return (
                      <tr
                        key={lineIdx}
                        onClick={() => lineNum > 0 && handleLineClick(file.path, lineNum)}
                        className={`
                          ${line.type === 'add' ? 'bg-green-900/30' : ''}
                          ${line.type === 'delete' ? 'bg-red-900/30' : ''}
                          ${selected ? 'bg-blue-900/50 ring-1 ring-blue-500' : ''}
                          ${threads.length > 0 ? 'border-l-2 border-yellow-500' : ''}
                          ${review ? 'cursor-pointer hover:bg-gray-800/50' : ''}
                        `}
                      >
                        <td className="w-12 text-right pr-2 text-gray-600 select-none">
                          {line.oldLineNum ?? ''}
                        </td>
                        <td className="w-12 text-right pr-4 text-gray-600 select-none border-r border-gray-700">
                          {line.newLineNum ?? ''}
                        </td>
                        <td className="pl-4 whitespace-pre">
                          <span
                            className={`
                              ${line.type === 'add' ? 'text-green-400' : ''}
                              ${line.type === 'delete' ? 'text-red-400' : ''}
                            `}
                          >
                            {line.type === 'add' && '+'}
                            {line.type === 'delete' && '-'}
                            {line.type === 'context' && ' '}
                            {line.content}
                          </span>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          ))}
        </div>
      ))}

      {/* Comment input popover */}
      {selectedLines && review && (
        <div className="fixed bottom-4 right-4 bg-gray-800 border border-gray-600 rounded-lg shadow-xl p-4 w-96">
          <div className="text-sm text-gray-400 mb-2">
            Comment on {selectedLines.file} lines {selectedLines.start}-{selectedLines.end}
          </div>
          <textarea
            value={commentText}
            onChange={(e) => setCommentText(e.target.value)}
            placeholder="Add your comment..."
            className="w-full bg-gray-900 border border-gray-700 rounded p-2 text-sm resize-none"
            rows={3}
            autoFocus
          />
          <div className="flex justify-end gap-2 mt-2">
            <button
              onClick={() => {
                setSelectedLines(null);
                setCommentText('');
              }}
              className="px-3 py-1 text-sm text-gray-400 hover:text-gray-200"
            >
              Cancel
            </button>
            <button
              onClick={handleSubmitComment}
              disabled={!commentText.trim() || submitting}
              className="px-3 py-1 text-sm bg-blue-600 hover:bg-blue-700 rounded disabled:opacity-50"
            >
              {submitting ? 'Adding...' : 'Add Comment'}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
