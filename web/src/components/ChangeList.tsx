import { useRef, useEffect, useState, forwardRef, useMemo } from 'react';
import { useAppContext } from '../context';
import { useChanges, useSessionChanges, mergeSessionAction, type Change } from '../hooks';
import type { GraphRow, PadLine } from '../types';
import { GraphLane, COL_WIDTH } from './GraphLane';

// Negative margin on the graph container bridges the 1px divide-y borders
// so SVG lines connect between adjacent rows.
const GRAPH_OVERLAP = 1; // px, must match divide-y border width

interface ChangeItemProps {
  change: Change;
  isSelected: boolean;
  focused: boolean;
  isMain: boolean;
  onClick: () => void;
}

const ChangeItem = forwardRef<HTMLButtonElement, ChangeItemProps>(function ChangeItem(
  { change, isSelected, focused, isMain, onClick },
  ref
) {
  return (
    <button
      ref={ref}
      onClick={onClick}
      className={`w-full h-full text-left px-2 flex flex-col justify-center overflow-hidden transition-colors ${
        isSelected && focused
          ? 'bg-blue-100 hover:bg-blue-100'
          : isSelected
            ? 'bg-blue-50 hover:bg-blue-100'
            : change.merged
              ? 'hover:bg-gray-100 opacity-60'
              : 'hover:bg-gray-100'
      }`}
    >
      <div className="flex items-center gap-1.5">
        {change.conflict && (
          <span className="font-mono text-xs font-bold text-red-600">x</span>
        )}
        {change.is_working_copy && (
          <span className="font-mono text-xs font-bold text-blue-600">@</span>
        )}
        <span className="font-mono text-xs text-gray-400">{change.change_id.slice(0, 8)}</span>
        {isMain && (
          <span className="text-xs bg-green-100 text-green-700 px-1 py-0.5 rounded font-medium leading-none">
            main
          </span>
        )}
        {change.empty && (
          <span className="text-xs bg-gray-200 text-gray-500 px-1 py-0.5 rounded leading-none">
            empty
          </span>
        )}
        <span className="flex-1" />
        {change.open_thread_count > 0 && (
          <span className="text-xs bg-amber-100 text-amber-700 px-1 py-0.5 rounded leading-none" title="Open threads">
            {change.open_thread_count}
          </span>
        )}
      </div>
      <div className="text-sm truncate max-w-full">
        {change.description?.split('\n')[0] || <span className="text-gray-400 italic">(no description)</span>}
      </div>
    </button>
  );
});

interface DisplayItem {
  type: 'change';
  change: Change;
  graphRow?: GraphRow;
  prevPadLines?: PadLine[];
}

// --- Session view ---

function SessionChangeList({ sessionName }: { sessionName: string }) {
  const { focusedPanel, selectedChangeId, selectChange, selectSession, selectedSessionVersion, selectSessionVersion } = useAppContext();
  const focused = focusedPanel === 'changes';
  const { sessions } = useChanges();
  const selectedVersion = selectedSessionVersion;
  const setSelectedVersion = selectSessionVersion;

  const session = sessions.find(s => s.name === sessionName);

  // Convert selectedVersion to API version string
  // UI shows pushes reversed (newest first), API uses 0-indexed from oldest
  const apiVersion = useMemo(() => {
    if (selectedVersion === 'live' || selectedVersion === 'latest') return selectedVersion;
    const reversedIdx = parseInt(selectedVersion, 10);
    if (!session || isNaN(reversedIdx)) return 'live';
    // Convert reversed index to original index (0 = oldest in API)
    return String(session.pushes.length - 1 - reversedIdx);
  }, [selectedVersion, session]);

  const sessionData = useSessionChanges(sessionName, apiVersion);
  const changes = sessionData?.changes ?? [];
  const graph = sessionData?.graph ?? [];

  const changeRefs = useRef<Map<string, HTMLButtonElement>>(new Map());

  useEffect(() => {
    if (selectedChangeId && focused) {
      const el = changeRefs.current.get(selectedChangeId);
      el?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  }, [selectedChangeId, focused]);

  const maxCols = useMemo(
    () => Math.max(1, ...graph.map(r => r.node_line.length)),
    [graph]
  );
  const graphWidth = maxCols * COL_WIDTH;

  const graphByChange = useMemo(() => {
    const byChange = new Map<string, GraphRow>();
    const prevPad = new Map<string, PadLine[]>();
    for (let gi = 0; gi < graph.length; gi++) {
      byChange.set(graph[gi].node, graph[gi]);
      if (gi > 0) prevPad.set(graph[gi].node, graph[gi - 1].pad_lines);
    }
    return { byChange, prevPad };
  }, [graph]);

  // Parse base session name for "Based on" link
  const baseSessionName = session?.base_bookmark.startsWith('session/')
    ? session.base_bookmark.replace('session/', '')
    : null;

  return (
    <div className="overflow-hidden">
      {/* Session header with version selector */}
      <div className="border-b border-gray-200 bg-white">
        <div className="px-3 pt-2 pb-1">
          <div className="font-medium text-sm">{sessionName}</div>
        </div>
        {/* Version selector: live + pushes */}
        <div className="max-h-24 overflow-y-auto">
          <button
            onClick={() => setSelectedVersion('live')}
            className={`w-full text-left text-xs px-3 py-1 truncate ${
              selectedVersion === 'live'
                ? 'bg-blue-100 text-blue-700 font-medium'
                : 'text-gray-500 hover:bg-gray-100'
            }`}
          >
            live
          </button>
          {session && [...session.pushes].reverse().map((push, i) => (
            <button
              key={push.commit_id}
              onClick={() => setSelectedVersion(String(i))}
              className={`w-full text-left text-xs px-3 py-1 truncate ${
                selectedVersion === String(i)
                  ? 'bg-blue-100 text-blue-700 font-medium'
                  : 'text-gray-500 hover:bg-gray-100'
              }`}
              title={`${push.summary}\n${new Date(push.timestamp).toLocaleString()}\n${push.change_count} changes`}
            >
              {push.summary}
            </button>
          ))}
        </div>
      </div>

      {/* Changes list with DAG graph */}
      <div className="divide-y divide-gray-200">
        {changes.map(change => {
          const isSelected = selectedChangeId === change.change_id;
          const graphRow = graphByChange.byChange.get(change.change_id);
          const prevPadLines = graphByChange.prevPad.get(change.change_id);

          return (
            <div key={change.change_id} className="flex items-stretch">
              <div className="flex-shrink-0 relative" style={{ width: graphWidth }}>
                {graphRow && (
                  <div className="absolute z-10" style={{ top: -GRAPH_OVERLAP, bottom: -GRAPH_OVERLAP, left: 0, right: 0 }}>
                    <GraphLane row={graphRow} prevPadLines={prevPadLines} />
                  </div>
                )}
              </div>
              <div className="flex-1 min-w-0">
                <ChangeItem
                  ref={(el) => {
                    if (el) changeRefs.current.set(change.change_id, el);
                    else changeRefs.current.delete(change.change_id);
                  }}
                  change={change}
                  isSelected={isSelected}
                  focused={focused}
                  isMain={false}
                  onClick={() => selectChange(change.change_id)}
                />
              </div>
            </div>
          );
        })}
      </div>

      {/* "Based on" link + merge */}
      {session && (
        <SessionFooter
          session={session}
          sessionName={sessionName}
          baseSessionName={baseSessionName}
          selectSession={selectSession}
        />
      )}
    </div>
  );
}

function SessionFooter({
  session,
  sessionName,
  baseSessionName,
  selectSession,
}: {
  session: { pushes: Array<{ commit_id: string; change_count: number }>; base_bookmark: string };
  sessionName: string;
  baseSessionName: string | null;
  selectSession: (name: string | null) => void;
}) {
  const { selectedSessionVersion } = useAppContext();
  const [merging, setMerging] = useState(false);

  // Always check live state for merge eligibility, independent of selected version
  const liveData = useSessionChanges(sessionName, 'live');
  const liveChanges = liveData?.changes ?? [];
  const latestPush = session.pushes[session.pushes.length - 1];
  const liveMatchesPushed = latestPush && liveChanges.length > 0 &&
    latestPush.commit_id === liveChanges[0].commit_id;

  // Only show "behind" on live or latest push (not old historical pushes)
  const isLiveOrLatest = selectedSessionVersion === 'live' || selectedSessionVersion === '0';
  const baseBehind = isLiveOrLatest &&
    liveData != null &&
    liveData.base_commit_id != null &&
    liveData.base_current_commit_id != null &&
    liveData.base_commit_id !== liveData.base_current_commit_id;

  const canMerge = liveMatchesPushed && !baseBehind;

  const handleMerge = async () => {
    if (!confirm(`Merge session "${sessionName}" into its base?`)) return;
    setMerging(true);
    try {
      const result = await mergeSessionAction(sessionName);
      if (!result.success) alert(result.message);
    } catch (err) {
      alert(`Merge failed: ${err}`);
    } finally {
      setMerging(false);
    }
  };

  // Show base commit from live data
  const baseCommitShort = liveData?.base_commit_id?.slice(0, 12);

  return (
    <div className="border-t border-gray-200">
      <div className="px-3 py-2 text-xs text-gray-500">
        Based on:{' '}
        {baseSessionName ? (
          <button
            className="text-blue-600 hover:underline"
            onClick={() => selectSession(baseSessionName)}
          >
            session/{baseSessionName} ↗
          </button>
        ) : (
          <button
            className="text-blue-600 hover:underline"
            onClick={() => selectSession(null)}
          >
            {session.base_bookmark} ↗
          </button>
        )}
        {baseCommitShort && (
          <span className="ml-1 font-mono text-gray-400">{baseCommitShort}</span>
        )}
        {baseBehind && (
          <span className="ml-1 text-amber-600">· behind</span>
        )}
      </div>
      <div className="px-3 pb-2">
        <button
          onClick={handleMerge}
          disabled={merging || !canMerge}
          className={`w-full px-3 py-1.5 text-sm rounded font-medium transition-colors ${
            canMerge
              ? 'bg-green-600 text-white hover:bg-green-700'
              : 'bg-gray-100 text-gray-400 cursor-not-allowed'
          } disabled:opacity-50`}
          title={
            baseBehind ? 'Base has moved — pull to update before merging'
              : !liveMatchesPushed ? 'Push changes before merging'
              : `Merge ${sessionName} into ${session.base_bookmark}`
          }
        >
          {merging ? 'Merging...' : 'Merge'}
        </button>
        {baseBehind && (
          <p className="text-xs text-amber-600 mt-1">Base has moved — pull to update</p>
        )}
        {!baseBehind && !liveMatchesPushed && (
          <p className="text-xs text-gray-400 mt-1">Push changes before merging</p>
        )}
      </div>
    </div>
  );
}

// --- Main view (DAG graph) ---

function MainChangeList() {
  const { changes, graph } = useChanges();
  const { focusedPanel, selectedChangeId, selectChange } = useAppContext();
  const focused = focusedPanel === 'changes';
  const changeRefs = useRef<Map<string, HTMLButtonElement>>(new Map());

  useEffect(() => {
    if (selectedChangeId && focused) {
      const el = changeRefs.current.get(selectedChangeId);
      el?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  }, [selectedChangeId, focused]);

  const maxCols = useMemo(
    () => Math.max(1, ...graph.map(r => r.node_line.length)),
    [graph]
  );
  const graphWidth = maxCols * COL_WIDTH;

  if (changes.length === 0) {
    return <div className="p-3 text-gray-400 text-sm">No changes found</div>;
  }

  const mainChangeId = changes.find(c => c.merged)?.change_id;

  const graphByChange = new Map<string, GraphRow>();
  const prevPadByChange = new Map<string, PadLine[]>();
  for (let gi = 0; gi < graph.length; gi++) {
    graphByChange.set(graph[gi].node, graph[gi]);
    if (gi > 0) prevPadByChange.set(graph[gi].node, graph[gi - 1].pad_lines);
  }

  const items: DisplayItem[] = changes.map(change => ({
    type: 'change' as const,
    change,
    graphRow: graphByChange.get(change.change_id),
    prevPadLines: prevPadByChange.get(change.change_id),
  }));

  return (
    <div className="overflow-hidden">
      <div className="divide-y divide-gray-200">
        {items.map((item) => {
          const { change, graphRow, prevPadLines } = item;
          const isSelected = selectedChangeId === change.change_id;
          const isMain = change.change_id === mainChangeId;
          return (
            <div key={change.change_id} className="flex items-stretch">
              <div className="flex-shrink-0 relative" style={{ width: graphWidth }}>
                {graphRow && (
                  <div className="absolute z-10" style={{ top: -GRAPH_OVERLAP, bottom: -GRAPH_OVERLAP, left: 0, right: 0 }}>
                    <GraphLane row={graphRow} prevPadLines={prevPadLines} />
                  </div>
                )}
              </div>
              <div className="flex-1 min-w-0">
                <ChangeItem
                  ref={(el) => {
                    if (el) changeRefs.current.set(change.change_id, el);
                    else changeRefs.current.delete(change.change_id);
                  }}
                  change={change}
                  isSelected={isSelected}
                  focused={focused}
                  isMain={isMain}
                  onClick={() => selectChange(change.change_id)}
                />
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// --- Public entry point ---

export function ChangeList() {
  const { selectedSessionName } = useAppContext();

  if (selectedSessionName) {
    return <SessionChangeList sessionName={selectedSessionName} />;
  }

  return <MainChangeList />;
}
