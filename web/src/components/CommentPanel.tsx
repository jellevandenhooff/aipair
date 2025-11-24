import { Review, Thread } from '../api';

interface Props {
  review: Review;
  onReviewUpdate: (review: Review) => void;
}

export function CommentPanel({ review }: Props) {
  if (review.threads.length === 0) {
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
        <h3 className="font-semibold text-sm text-gray-700">
          Comments ({review.threads.length})
        </h3>
      </div>

      {openThreads.length > 0 && (
        <div className="p-4">
          <h4 className="text-xs font-semibold text-amber-600 uppercase tracking-wide mb-3">
            Open ({openThreads.length})
          </h4>
          <div className="space-y-4">
            {openThreads.map((thread) => (
              <ThreadCard key={thread.id} thread={thread} />
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
              <ThreadCard key={thread.id} thread={thread} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function ThreadCard({ thread }: { thread: Thread }) {
  return (
    <div className="bg-white border border-gray-200 rounded-lg p-3 shadow-sm">
      <div className="text-xs text-gray-400 mb-2 font-mono">
        {thread.file}:{thread.line_start}-{thread.line_end}
        <span className="ml-2 text-gray-300">[{thread.id}]</span>
      </div>

      <div className="space-y-2">
        {thread.comments.map((comment, idx) => (
          <div
            key={idx}
            className={`text-sm ${
              comment.author === 'claude'
                ? 'bg-purple-50 border-l-2 border-purple-400 pl-2'
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
