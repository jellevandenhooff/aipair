import { useEffect, useState } from 'react';
import { fetchTimeline, type TimelineEntry } from '../api';

function formatTime(timestamp: string): string {
  const date = new Date(timestamp);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMins = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMins / 60);
  const diffDays = Math.floor(diffHours / 24);

  if (diffMins < 1) return 'just now';
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  if (diffDays < 7) return `${diffDays}d ago`;
  return date.toLocaleDateString();
}

function formatFullTime(timestamp: string): string {
  return new Date(timestamp).toLocaleString();
}

function ChatMessageEvent({ entry }: { entry: TimelineEntry }) {
  const isUser = entry.author === 'user';
  const text = entry.text || '';
  // Truncate long messages
  const truncated = text.length > 500 ? text.slice(0, 500) + '...' : text;

  return (
    <div className={`flex ${isUser ? 'justify-start' : 'justify-end'}`}>
      <div
        className={`max-w-[75%] rounded-lg px-3 py-2 text-sm ${
          isUser
            ? 'bg-gray-100 text-gray-900'
            : 'bg-blue-50 text-gray-900'
        }`}
      >
        <div className="flex items-center gap-2 mb-1">
          <span className="font-medium text-xs text-gray-500">
            {isUser ? 'You' : 'Claude'}
          </span>
          <span className="text-xs text-gray-400" title={formatFullTime(entry.timestamp)}>
            {formatTime(entry.timestamp)}
          </span>
          {entry.session_id && (
            <span className="text-xs text-gray-300" title={entry.session_id}>
              session
            </span>
          )}
        </div>
        <div className="whitespace-pre-wrap break-words">{truncated}</div>
      </div>
    </div>
  );
}

function ReviewCommentEvent({ entry }: { entry: TimelineEntry }) {
  return (
    <div className="flex justify-center">
      <div className="bg-yellow-50 border border-yellow-200 rounded-lg px-3 py-2 text-sm max-w-[85%]">
        <div className="flex items-center gap-2 mb-1">
          <span className="text-yellow-700 font-medium text-xs">Review Comment</span>
          <span className="text-xs text-gray-400" title={formatFullTime(entry.timestamp)}>
            {formatTime(entry.timestamp)}
          </span>
        </div>
        <div className="text-xs text-gray-500 mb-1">
          {entry.file}:{entry.line_start}
          {entry.line_end !== entry.line_start ? `-${entry.line_end}` : ''}
          <span className="ml-2 text-gray-400">{entry.change_id?.slice(0, 8)}</span>
        </div>
        <div className="text-gray-800">{entry.text}</div>
      </div>
    </div>
  );
}

function ReviewReplyEvent({ entry }: { entry: TimelineEntry }) {
  const isUser = entry.author === 'user';
  return (
    <div className="flex justify-center">
      <div className="bg-orange-50 border border-orange-200 rounded-lg px-3 py-2 text-sm max-w-[85%]">
        <div className="flex items-center gap-2 mb-1">
          <span className="text-orange-700 font-medium text-xs">
            {isUser ? 'You' : 'Claude'} replied
          </span>
          <span className="text-xs text-gray-400" title={formatFullTime(entry.timestamp)}>
            {formatTime(entry.timestamp)}
          </span>
          <span className="text-xs text-gray-400">
            thread {entry.thread_id?.slice(0, 8)}
          </span>
        </div>
        <div className="text-gray-800">{entry.text}</div>
      </div>
    </div>
  );
}

function CodeSnapshotEvent({ entry }: { entry: TimelineEntry }) {
  return (
    <div className="flex justify-center">
      <div className="bg-green-50 border border-green-200 rounded-lg px-3 py-2 text-sm">
        <div className="flex items-center gap-2">
          <span className="text-green-700 font-medium text-xs">Revision</span>
          <span className="text-xs text-gray-400" title={formatFullTime(entry.timestamp)}>
            {formatTime(entry.timestamp)}
          </span>
        </div>
        <div className="text-gray-800 mt-1">
          <span className="font-mono text-xs text-gray-500">{entry.change_id?.slice(0, 8)}</span>
          {' '}{entry.description}
        </div>
      </div>
    </div>
  );
}

function TimelineEvent({ entry }: { entry: TimelineEntry }) {
  switch (entry.type) {
    case 'ChatMessage':
      return <ChatMessageEvent entry={entry} />;
    case 'ReviewComment':
      return <ReviewCommentEvent entry={entry} />;
    case 'ReviewReply':
      return <ReviewReplyEvent entry={entry} />;
    case 'CodeSnapshot':
      return <CodeSnapshotEvent entry={entry} />;
    default:
      return null;
  }
}

export function TimelineView() {
  const [entries, setEntries] = useState<TimelineEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetchTimeline()
      .then((data) => {
        setEntries(data);
        setLoading(false);
      })
      .catch((err) => {
        setError(err.message);
        setLoading(false);
      });
  }, []);

  if (loading) {
    return (
      <div className="h-screen flex items-center justify-center text-gray-400">
        Loading timeline...
      </div>
    );
  }

  if (error) {
    return (
      <div className="h-screen flex items-center justify-center text-red-500">
        Error: {error}
      </div>
    );
  }

  return (
    <div className="h-screen flex flex-col bg-white">
      <header className="border-b border-gray-200 px-4 py-3 flex items-center justify-between">
        <h1 className="text-lg font-semibold text-gray-900">Development Timeline</h1>
        <span className="text-sm text-gray-400">{entries.length} events</span>
      </header>
      <div className="flex-1 overflow-y-auto px-4 py-4">
        {entries.length === 0 ? (
          <div className="text-center text-gray-400 mt-10">
            No timeline events yet. Add review comments or chat with Claude to populate the timeline.
          </div>
        ) : (
          <div className="max-w-3xl mx-auto space-y-3">
            {entries.map((entry, i) => (
              <TimelineEvent key={`${entry.timestamp}-${i}`} entry={entry} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
