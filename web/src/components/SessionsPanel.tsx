import { useState } from 'react';
import { useAppContext } from '../context';
import { useChanges, mergeSessionAction, type SessionSummary } from '../hooks';

export function SessionsPanel() {
  const { focusedPanel, setFocusedPanel } = useAppContext();
  const isFocused = focusedPanel === 'sessions';
  const { sessions } = useChanges();
  const [merging, setMerging] = useState<string | null>(null);

  const handleMerge = async (name: string) => {
    setMerging(name);
    try {
      const result = await mergeSessionAction(name);
      if (!result.success) {
        alert(result.message);
      }
    } catch (e) {
      alert(`Merge failed: ${e}`);
    } finally {
      setMerging(null);
    }
  };

  const activeSessions = sessions.filter((s: SessionSummary) => s.status === 'active');
  const mergedSessions = sessions.filter((s: SessionSummary) => s.status === 'merged');

  return (
    <div
      className={`border-t border-gray-200 bg-gray-50 flex flex-col ${isFocused ? 'ring-1 ring-blue-300 ring-inset' : ''}`}
      style={{ height: '180px' }}
      onClick={() => setFocusedPanel('sessions')}
    >
      {/* Header */}
      <div className="flex items-center px-3 py-1.5 border-b border-gray-200 bg-gray-100">
        <span className="text-xs font-semibold text-gray-500 uppercase tracking-wide">Sessions</span>
        <span className="ml-2 text-xs text-gray-400">{activeSessions.length} active</span>
        <span className="flex-1" />
        {isFocused && (
          <span className="text-xs text-gray-400">Tab to cycle panels</span>
        )}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto">
        {sessions.length === 0 ? (
          <div className="flex items-center justify-center h-full text-sm text-gray-400">
            No sessions
          </div>
        ) : (
          <div className="divide-y divide-gray-200">
            {activeSessions.map((s: SessionSummary) => (
              <div key={s.name} className="flex items-center px-3 py-2 gap-2">
                <span className="text-xs bg-green-100 text-green-700 px-1.5 py-0.5 rounded font-medium">active</span>
                <span className="text-sm font-medium text-gray-800 flex-1">{s.name}</span>
                <span className="text-xs text-gray-400">{s.push_count} push{s.push_count !== 1 ? 'es' : ''}</span>
                {s.last_push && (
                  <span className="text-xs text-gray-400 truncate max-w-[120px]" title={s.last_push}>{s.last_push}</span>
                )}
                <button
                  onClick={(e) => { e.stopPropagation(); handleMerge(s.name); }}
                  disabled={merging === s.name}
                  className="text-xs px-2 py-0.5 bg-blue-500 text-white rounded hover:bg-blue-600 disabled:opacity-50"
                >
                  {merging === s.name ? 'Merging...' : 'Merge'}
                </button>
              </div>
            ))}
            {mergedSessions.map((s: SessionSummary) => (
              <div key={s.name} className="flex items-center px-3 py-2 gap-2 opacity-50">
                <span className="text-xs bg-gray-200 text-gray-500 px-1.5 py-0.5 rounded">merged</span>
                <span className="text-sm text-gray-500 flex-1">{s.name}</span>
                <span className="text-xs text-gray-400">{s.push_count} push{s.push_count !== 1 ? 'es' : ''}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
