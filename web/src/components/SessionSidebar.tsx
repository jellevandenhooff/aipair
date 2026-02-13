import { useAppContext } from '../context';
import { useChanges, type SessionSummary } from '../hooks';

export function SessionSidebar() {
  const { selectedSessionName, selectSession } = useAppContext();
  const { sessions } = useChanges();

  // Build a tree: sessions with base_bookmark starting with "session/" are children
  const sessionsByBookmark = new Map<string, SessionSummary>();
  for (const s of sessions) {
    sessionsByBookmark.set(`session/${s.name}`, s);
  }

  // Determine indent level: session → parent → grandparent → ...
  const getDepth = (s: SessionSummary): number => {
    let depth = 0;
    let current = s;
    while (current.base_bookmark.startsWith('session/')) {
      depth++;
      const parentName = current.base_bookmark.replace('session/', '');
      const parent = sessions.find(p => p.name === parentName);
      if (!parent) break;
      current = parent;
    }
    return depth;
  };

  // Sort sessions into tree order: parents before children, depth-first
  const sortTreeOrder = (list: SessionSummary[]): SessionSummary[] => {
    const byParent = new Map<string, SessionSummary[]>();
    for (const s of list) {
      const parentKey = s.base_bookmark.startsWith('session/')
        ? s.base_bookmark.replace('session/', '')
        : '__root__';
      const children = byParent.get(parentKey) ?? [];
      children.push(s);
      byParent.set(parentKey, children);
    }
    const result: SessionSummary[] = [];
    const visit = (parentKey: string) => {
      for (const s of byParent.get(parentKey) ?? []) {
        result.push(s);
        visit(s.name);
      }
    };
    visit('__root__');
    // Append any orphans (parent not in list)
    for (const s of list) {
      if (!result.includes(s)) result.push(s);
    }
    return result;
  };

  const activeSessions = sortTreeOrder(sessions.filter(s => s.status === 'active'));
  const mergedSessions = sessions.filter(s => s.status === 'merged');

  return (
    <aside className="w-48 border-r border-gray-200 bg-gray-50 flex flex-col overflow-y-auto">
      <div className="px-2 py-2 border-b border-gray-200">
        <span className="text-xs font-semibold text-gray-500 uppercase tracking-wide">Sessions</span>
      </div>

      {/* Main entry */}
      <button
        onClick={() => selectSession(null)}
        className={`w-full text-left px-2 py-1.5 text-sm flex items-center gap-1.5 transition-colors ${
          selectedSessionName === null
            ? 'bg-blue-100 font-medium'
            : 'hover:bg-gray-100'
        }`}
      >
        <span>main</span>
      </button>

      {/* Active sessions */}
      {activeSessions.map(s => {
        const depth = getDepth(s);
        const isSelected = selectedSessionName === s.name;
        return (
          <button
            key={s.name}
            onClick={() => selectSession(s.name)}
            className={`w-full text-left py-1.5 pr-2 text-sm flex items-center gap-1.5 transition-colors ${
              isSelected
                ? 'bg-blue-100 font-medium'
                : 'hover:bg-gray-100'
            }`}
            style={{ paddingLeft: `${8 + (depth + 1) * 12}px` }}
          >
            <span className="truncate flex-1">{s.name}</span>
            <span className="text-xs text-gray-400">{s.change_count}</span>
          </button>
        );
      })}

      {/* Merged sessions (collapsed) */}
      {mergedSessions.length > 0 && (
        <>
          <div className="px-2 py-1 mt-2 border-t border-gray-200">
            <span className="text-xs text-gray-400">Merged</span>
          </div>
          {mergedSessions.map(s => (
            <div
              key={s.name}
              className="w-full text-left px-2 py-1 text-sm text-gray-400 flex items-center gap-1.5"
            >
              <span className="truncate">{s.name}</span>
            </div>
          ))}
        </>
      )}
    </aside>
  );
}
