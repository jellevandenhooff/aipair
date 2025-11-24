import { useRef, useEffect, forwardRef } from 'react';
import { Thread } from '../api';
import { useAppStore } from '../store';

export function CommentPanel() {
  const review = useAppStore((s) => s.review);
  const focused = useAppStore((s) => s.focusedPanel === 'threads');
  const selectedThreadId = useAppStore((s) => s.selectedThreadId);
  const replyingToThread = useAppStore((s) => s.replyingToThread);
  const startReply = useAppStore((s) => s.startReply);

  const replyInputRef = useRef<HTMLTextAreaElement>(null);
  const threadRefs = useRef<Map<string, HTMLDivElement>>(new Map());

  // Scroll selected thread into view when it changes
  useEffect(() => {
    if (selectedThreadId && focused) {
      const el = threadRefs.current.get(selectedThreadId);
      el?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  }, [selectedThreadId, focused]);

  // Focus reply input when 'r' is pressed and panel is focused
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLInputElement) return;
      if (!focused || !selectedThreadId) return;

      if (e.key === 'r') {
        e.preventDefault();
        startReply(selectedThreadId);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [focused, selectedThreadId, startReply]);

  // Focus textarea when replying starts
  useEffect(() => {
    if (replyingToThread && focused && replyInputRef.current) {
      replyInputRef.current.focus();
    }
  }, [replyingToThread, focused]);

  if (!review || review.threads.length === 0) {
    return (
      <div className="p-4 text-gray-400 text-sm">
        No comments yet. Click on lines in the diff to add comments.
      </div>
    );
  }

  const openThreads = review.threads.filter((t) => t.status === 'open');
  const resolvedThreads = review.threads.filter((t) => t.status === 'resolved');

  return (
    <div className="divide-y divide-gray-200">
      <div className="p-4">
        <h3 className="font-semibold text-sm text-gray-700">Comments ({review.threads.length})</h3>
        <p className="text-xs text-gray-400 mt-1">
          {focused ? 'j/k: navigate | r: reply | x: resolve' : 'Tab to focus'}
        </p>
      </div>

      {openThreads.length > 0 && (
        <div className="p-4">
          <h4 className="text-xs font-semibold text-amber-600 uppercase tracking-wide mb-3">
            Open ({openThreads.length})
          </h4>
          <div className="space-y-4">
            {openThreads.map((thread) => (
              <ThreadCard
                key={thread.id}
                ref={(el) => {
                  if (el) threadRefs.current.set(thread.id, el);
                  else threadRefs.current.delete(thread.id);
                }}
                thread={thread}
                replyInputRef={selectedThreadId === thread.id ? replyInputRef : undefined}
              />
            ))}
          </div>
        </div>
      )}

      {resolvedThreads.length > 0 && (
        <div className="p-4">
          <h4 className="text-xs font-semibold text-green-600 uppercase tracking-wide mb-3">
            Resolved ({resolvedThreads.length})
          </h4>
          <div className="space-y-4 opacity-60">
            {resolvedThreads.map((thread) => (
              <ThreadCard
                key={thread.id}
                ref={(el) => {
                  if (el) threadRefs.current.set(thread.id, el);
                  else threadRefs.current.delete(thread.id);
                }}
                thread={thread}
                replyInputRef={undefined}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

interface ThreadCardProps {
  thread: Thread;
  replyInputRef?: React.RefObject<HTMLTextAreaElement>;
}

const ThreadCard = forwardRef<HTMLDivElement, ThreadCardProps>(function ThreadCard(
  { thread, replyInputRef },
  ref
) {
  // Get state from store
  const focused = useAppStore((s) => s.focusedPanel === 'threads');
  const selectedThreadId = useAppStore((s) => s.selectedThreadId);
  const replyingToThread = useAppStore((s) => s.replyingToThread);
  const replyText = useAppStore((s) => s.replyText);
  const submittingReply = useAppStore((s) => s.submittingReply);

  const setSelectedThreadId = useAppStore((s) => s.setSelectedThreadId);
  const startReply = useAppStore((s) => s.startReply);
  const cancelReply = useAppStore((s) => s.cancelReply);
  const setReplyText = useAppStore((s) => s.setReplyText);
  const submitReply = useAppStore((s) => s.submitReply);
  const toggleThreadStatus = useAppStore((s) => s.toggleThreadStatus);

  const selected = selectedThreadId === thread.id;
  const replying = replyingToThread && selected && focused;

  return (
    <div
      ref={ref}
      onClick={() => setSelectedThreadId(thread.id)}
      className={`bg-white border rounded-lg p-3 shadow-sm cursor-pointer transition-colors ${
        selected && focused
          ? 'border-blue-500 ring-2 ring-blue-200'
          : selected
            ? 'border-blue-300'
            : 'border-gray-200 hover:border-gray-300'
      }`}
    >
      <div className="text-xs text-gray-400 mb-2 font-mono">
        {thread.file}:{thread.line_start}-{thread.line_end}
        <span className="ml-2 text-gray-300">[{thread.id}]</span>
      </div>

      <div className="space-y-2">
        {thread.comments.map((comment, idx) => (
          <div
            key={idx}
            className={`text-sm ${
              comment.author === 'claude' ? 'bg-purple-50 border-l-2 border-purple-400 pl-2' : ''
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

      {/* Actions for selected thread */}
      {selected && (
        <div className="mt-3 pt-3 border-t border-gray-100">
          {thread.status === 'resolved' ? (
            <button
              onClick={(e) => {
                e.stopPropagation();
                toggleThreadStatus(thread.id);
              }}
              className="px-2 py-1 text-xs text-amber-600 hover:text-amber-700 hover:bg-amber-50 rounded"
            >
              Reopen
            </button>
          ) : replying ? (
            <>
              <textarea
                ref={replyInputRef}
                value={replyText}
                onChange={(e) => setReplyText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && !e.shiftKey) {
                    e.preventDefault();
                    if (replyText.trim()) submitReply(thread.id);
                  } else if (e.key === 'Escape') {
                    cancelReply();
                  }
                }}
                placeholder="Reply... (Enter to send, Esc to cancel)"
                className="w-full bg-gray-50 border border-gray-200 rounded p-2 text-sm resize-none"
                rows={2}
              />
              <div className="flex justify-between items-center mt-2">
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
                    if (replyText.trim()) submitReply(thread.id);
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
                  toggleThreadStatus(thread.id);
                }}
                className="px-2 py-1 text-xs text-green-600 hover:text-green-700 hover:bg-green-50 rounded"
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
});
