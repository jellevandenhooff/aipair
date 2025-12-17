import { useState, useRef, useEffect, forwardRef } from 'react';
import { Thread, Revision } from '../types';
import { useAppContext } from '../context';
import { replyToThread, resolveThread, reopenThread, mergeChange, type Change, type Review } from '../hooks';

/** Displays a revision label: "base", "Pending abc1234", or "v3 abc1234" */
export function RevisionLabel({ revision, showHash = true }: { revision: Revision | 'base'; showHash?: boolean }) {
  if (revision === 'base') {
    return <span className="font-mono">base</span>;
  }
  if (revision.is_pending) {
    return (
      <span className="font-mono">
        Pending
        {showHash && <span className="ml-1 text-gray-400">{revision.commit_id.slice(0, 7)}</span>}
      </span>
    );
  }
  return (
    <span className="font-mono">
      v{revision.number}
      {showHash && <span className="ml-1 text-gray-400">{revision.commit_id.slice(0, 7)}</span>}
    </span>
  );
}

interface CommentPanelProps {
  review: Review;
  selectedChange: Change;
}

export function CommentPanel({ review, selectedChange }: CommentPanelProps) {
  const {
    focusedPanel,
    selectedThreadId,
    selectedRevision,
    replyingToThread,
    comparisonBase,
    startReply,
    selectRevision,
    setComparisonBase,
    navigateThreads,
  } = useAppContext();
  const threadsFocused = focusedPanel === 'threads';
  const revisionsFocused = focusedPanel === 'revisions';

  const [merging, setMerging] = useState(false);

  const replyInputRef = useRef<HTMLTextAreaElement>(null);
  const threadRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const revisionRefs = useRef<Map<number, HTMLButtonElement>>(new Map());

  // Scroll selected thread into view when it changes
  useEffect(() => {
    if (selectedThreadId && threadsFocused) {
      const el = threadRefs.current.get(selectedThreadId);
      el?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  }, [selectedThreadId, threadsFocused]);

  // Scroll selected revision into view when it changes
  useEffect(() => {
    if (selectedRevision && revisionsFocused) {
      const el = revisionRefs.current.get(selectedRevision.number);
      el?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  }, [selectedRevision, revisionsFocused]);

  // Keyboard navigation for revisions panel
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLInputElement) return;
      if (!revisionsFocused) return;

      // Get revisions in display order (reversed, newest first)
      const revisions = [...review.revisions].reverse();
      if (revisions.length === 0) return;

      // Find current index based on selectedRevision
      const validSelectedRevision = selectedRevision &&
        review.revisions.some(r => r.commit_id === selectedRevision.commit_id)
        ? selectedRevision
        : null;
      const effectiveRevision = validSelectedRevision ?? revisions[0];
      const currentIdx = revisions.findIndex(r => r.number === effectiveRevision?.number);

      if (e.key === 'j' || e.key === 'ArrowDown') {
        e.preventDefault();
        const nextIdx = Math.min(currentIdx + 1, revisions.length - 1);
        if (nextIdx !== currentIdx) {
          selectRevision(revisions[nextIdx]);
        }
        return;
      }
      if (e.key === 'k' || e.key === 'ArrowUp') {
        e.preventDefault();
        const nextIdx = Math.max(currentIdx - 1, 0);
        if (nextIdx !== currentIdx) {
          selectRevision(revisions[nextIdx]);
        }
        return;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [revisionsFocused, selectedRevision, review.revisions, selectRevision]);

  // Keyboard navigation for threads panel
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLInputElement) return;
      if (!threadsFocused) return;

      // Compute thread IDs for navigation (open first, then resolved sorted by revision)
      const openIds = review.threads.filter((t) => t.status === 'open').map((t) => t.id);
      const resolvedIds = review.threads
        .filter((t) => t.status === 'resolved')
        .slice()
        .sort((a, b) => {
          const revA = a.created_at_revision ?? 0;
          const revB = b.created_at_revision ?? 0;
          if (revB !== revA) return revB - revA;
          return b.id.localeCompare(a.id);
        })
        .map((t) => t.id);
      const threadIds = [...openIds, ...resolvedIds];

      // j/k navigation works even without a selected thread
      if (e.key === 'j' || e.key === 'ArrowDown') {
        e.preventDefault();
        if (threadIds.length > 0) {
          navigateThreads('down', threadIds);
        }
        return;
      }
      if (e.key === 'k' || e.key === 'ArrowUp') {
        e.preventDefault();
        if (threadIds.length > 0) {
          navigateThreads('up', threadIds);
        }
        return;
      }

      // These require a selected thread
      if (!selectedThreadId) return;

      if (e.key === 'r') {
        e.preventDefault();
        const thread = review.threads.find((t) => t.id === selectedThreadId);
        if (thread?.status === 'open') {
          startReply(selectedThreadId);
        }
      } else if (e.key === 'x') {
        e.preventDefault();
        const thread = review.threads.find((t) => t.id === selectedThreadId);
        if (thread) {
          if (thread.status === 'open') {
            resolveThread(selectedChange.change_id, selectedThreadId);
          } else {
            reopenThread(selectedChange.change_id, selectedThreadId);
          }
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [threadsFocused, selectedThreadId, startReply, navigateThreads, review.threads, selectedChange.change_id]);

  // Focus textarea when replying starts
  useEffect(() => {
    if (replyingToThread && threadsFocused && replyInputRef.current) {
      replyInputRef.current.focus();
    }
  }, [replyingToThread, threadsFocused]);

  const handleMerge = async (force = false) => {
    setMerging(true);
    try {
      const result = await mergeChange(selectedChange.change_id, force);
      if (!result.success) {
        console.error('Merge failed:', result.message);
      }
    } catch (e) {
      console.error('Merge failed:', e);
    } finally {
      setMerging(false);
    }
  };

  const handleCompareRevisions = (fromRev: Revision | null, toRev: Revision | null) => {
    selectRevision(toRev);
    setComparisonBase(fromRev);
  };

  const openThreads = review.threads.filter((t) => t.status === 'open');
  // Sort resolved threads by revision (newest first), then by id for stable ordering
  const resolvedThreads = review.threads
    .filter((t) => t.status === 'resolved')
    .slice()
    .sort((a, b) => {
      const revA = a.created_at_revision ?? 0;
      const revB = b.created_at_revision ?? 0;
      if (revB !== revA) return revB - revA;
      return b.id.localeCompare(a.id); // stable secondary sort
    });
  const totalThreads = review.threads.length;
  const hasOpenThreads = openThreads.length > 0;
  const hasNoDescription = !selectedChange.description?.trim();

  return (
    <div className="divide-y divide-gray-200">
      {/* Merge section */}
      <div className="p-4">
        {selectedChange.merged ? (
          <div className="text-sm text-green-600 font-medium">✓ Merged</div>
        ) : (
          <>
            <button
              onClick={() => handleMerge(false)}
              disabled={merging || hasNoDescription}
              className={`w-full px-4 py-2 rounded font-medium transition-colors ${
                hasOpenThreads || selectedChange.has_pending_changes || hasNoDescription
                  ? 'bg-gray-100 text-gray-500 hover:bg-gray-200'
                  : 'bg-green-600 text-white hover:bg-green-700'
              } disabled:opacity-50`}
            >
              {merging ? 'Merging...' : 'Merge'}
            </button>
            {hasNoDescription && (
              <p className="text-xs text-red-600 mt-2">
                No commit message. Use <code className="bg-gray-100 px-1 rounded">jj describe -m "..."</code> to set one.
              </p>
            )}
            {!hasNoDescription && selectedChange.has_pending_changes && (
              <p className="text-xs text-blue-600 mt-2">
                Pending changes not recorded as a revision.{' '}
                <button
                  onClick={() => handleMerge(true)}
                  disabled={merging}
                  className="underline hover:text-blue-700"
                >
                  Force merge
                </button>
              </p>
            )}
            {!hasNoDescription && hasOpenThreads && !selectedChange.has_pending_changes && (
              <p className="text-xs text-amber-600 mt-2">
                {openThreads.length} open thread{openThreads.length !== 1 ? 's' : ''} remaining.{' '}
                <button
                  onClick={() => handleMerge(true)}
                  disabled={merging}
                  className="underline hover:text-amber-700"
                >
                  Force merge
                </button>
              </p>
            )}
          </>
        )}
      </div>

      {/* Revisions section - always show when review has revisions */}
      {review.revisions.length > 0 && (
        <div className={`p-4 ${revisionsFocused ? 'bg-blue-50/50' : ''}`}>
          <h3 className="font-semibold text-sm text-gray-700 mb-2">
            Revisions ({review.revisions.filter(r => !r.is_pending).length})
          </h3>
          <div className="space-y-1">
            {[...review.revisions].reverse().map((rev, idx, arr) => {
              const prevRev = arr[idx + 1]; // Previous revision in display order (lower number)
              // Validate selectedRevision belongs to this review (by commit_id)
              const validSelectedRevision = selectedRevision &&
                review.revisions.some(r => r.commit_id === selectedRevision.commit_id)
                ? selectedRevision
                : null;
              const effectiveSelected = validSelectedRevision ?? review.revisions[review.revisions.length - 1];
              const isSelected = effectiveSelected?.number === rev.number && !comparisonBase;
              const isComparing = prevRev &&
                comparisonBase?.number === prevRev.number &&
                effectiveSelected?.number === rev.number;

              return (
                <div key={`${review.change_id}-${rev.number}`} className="flex items-center gap-1">
                  <button
                    ref={(el) => {
                      if (el) revisionRefs.current.set(rev.number, el);
                      else revisionRefs.current.delete(rev.number);
                    }}
                    onClick={() => selectRevision(rev)}
                    className={`flex-1 text-left px-2 py-1 text-xs rounded transition-colors truncate ${
                      isSelected && revisionsFocused
                        ? 'bg-blue-100 text-blue-800 ring-2 ring-blue-300'
                        : isSelected
                          ? 'bg-blue-100 text-blue-800'
                          : rev.is_pending
                            ? 'hover:bg-gray-100 text-blue-600'
                            : 'hover:bg-gray-100 text-gray-600'
                    }`}
                  >
                    <RevisionLabel revision={rev} />
                    {!rev.is_pending && rev.description && (
                      <span className="ml-2">{rev.description}</span>
                    )}
                  </button>
                  {prevRev && !prevRev.is_pending && (
                    <button
                      onClick={() => handleCompareRevisions(prevRev, rev)}
                      title={`Compare v${prevRev.number} → ${rev.is_pending ? 'pending' : `v${rev.number}`}`}
                      className={`px-1.5 py-1 text-xs rounded transition-colors ${
                        isComparing
                          ? 'bg-purple-100 text-purple-800'
                          : 'hover:bg-gray-100 text-gray-400 hover:text-gray-600'
                      }`}
                    >
                      Δ
                    </button>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Comments header */}
      <div className={`p-4 ${threadsFocused ? 'bg-blue-50/50' : ''}`}>
        <h3 className="font-semibold text-sm text-gray-700">Comments ({totalThreads})</h3>
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
                changeId={review.change_id}
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
                changeId={review.change_id}
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
  changeId: string;
  replyInputRef?: React.RefObject<HTMLTextAreaElement>;
}

const ThreadCard = forwardRef<HTMLDivElement, ThreadCardProps>(function ThreadCard(
  { thread, changeId, replyInputRef },
  ref
) {
  // Get state from context
  const {
    focusedPanel,
    selectedThreadId,
    replyingToThread,
    replyText,
    submittingReply,
    setSelectedThreadId,
    startReply,
    cancelReply,
    setReplyText,
    setSubmittingReply,
  } = useAppContext();
  const threadsFocused = focusedPanel === 'threads';

  const selected = selectedThreadId === thread.id;
  const replying = replyingToThread && selected && threadsFocused;

  const handleSubmitReply = async () => {
    if (!replyText.trim()) return;
    setSubmittingReply(true);
    try {
      await replyToThread(changeId, thread.id, replyText.trim());
      setReplyText('');
      cancelReply();
    } catch (e) {
      console.error('Failed to reply:', e);
    } finally {
      setSubmittingReply(false);
    }
  };

  const handleToggleStatus = async () => {
    try {
      if (thread.status === 'open') {
        await resolveThread(changeId, thread.id);
      } else {
        await reopenThread(changeId, thread.id);
      }
    } catch (e) {
      console.error('Failed to toggle thread status:', e);
    }
  };

  return (
    <div
      ref={ref}
      onClick={() => setSelectedThreadId(thread.id)}
      className={`bg-white border rounded-lg p-3 shadow-sm cursor-pointer transition-colors ${
        selected && threadsFocused
          ? 'border-blue-500 ring-2 ring-blue-200'
          : selected
            ? 'border-blue-300'
            : 'border-gray-200 hover:border-gray-300'
      }`}
    >
      <div className="text-xs text-gray-400 mb-2 font-mono">
        {thread.file}:{thread.line_start}
        {thread.created_at_revision != null && (
          <span className="ml-2 text-gray-500">v{thread.created_at_revision}</span>
        )}
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
                handleToggleStatus();
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
                    handleSubmitReply();
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
                    handleSubmitReply();
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
                  handleToggleStatus();
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
