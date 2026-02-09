import { useRef, useEffect, useState, forwardRef, useMemo } from 'react';
import { useAppContext } from '../context';
import { useChanges, useTopics, type Change } from '../hooks';
import type { Topic, GraphRow, PadLine } from '../types';
import { GraphLane, COL_WIDTH } from './GraphLane';

// Negative margin on the graph container bridges the 1px divide-y borders
// so SVG lines connect between adjacent rows.
const GRAPH_OVERLAP = 1; // px, must match divide-y border width

// Stable color palette for topics
const TOPIC_COLORS = [
  '#3b82f6', // blue
  '#8b5cf6', // violet
  '#ec4899', // pink
  '#f59e0b', // amber
  '#10b981', // emerald
  '#06b6d4', // cyan
  '#f97316', // orange
  '#6366f1', // indigo
];

function getTopicColor(index: number): string {
  return TOPIC_COLORS[index % TOPIC_COLORS.length];
}


interface ChangeItemProps {
  change: Change;
  topicSlug?: string;
  topicColor?: string;
  isSelected: boolean;
  focused: boolean;
  isMain: boolean;
  onClick: () => void;
}

const ChangeItem = forwardRef<HTMLButtonElement, ChangeItemProps>(function ChangeItem(
  { change, topicSlug, topicColor, isSelected, focused, isMain, onClick },
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
        {topicSlug && topicColor && (
          <span className="text-xs font-medium" style={{ color: topicColor }}>{topicSlug}</span>
        )}
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

type DisplayItem =
  | { type: 'change'; change: Change; graphRow?: GraphRow; prevPadLines?: PadLine[] }
  | { type: 'gap'; count: number };

export function ChangeList() {
  const { changes, graph } = useChanges();
  const { topics: topicsList } = useTopics();

  const { focusedPanel, selectedChangeId, selectChange } = useAppContext();
  const focused = focusedPanel === 'changes';

  const [filterTopicId, setFilterTopicId] = useState<string | null>(null);

  const changeRefs = useRef<Map<string, HTMLButtonElement>>(new Map());

  useEffect(() => {
    if (selectedChangeId && focused) {
      const el = changeRefs.current.get(selectedChangeId);
      el?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  }, [selectedChangeId, focused]);

  const topicById = useMemo(() => {
    const map = new Map<string, Topic>();
    for (const t of topicsList) map.set(t.id, t);
    return map;
  }, [topicsList]);

  const topicColorMap = useMemo(() => {
    const sorted = [...topicsList]
      .filter(t => t.status !== 'finished')
      .sort((a, b) => a.created_at.localeCompare(b.created_at));
    const map = new Map<string, string>();
    sorted.forEach((t, i) => map.set(t.id, getTopicColor(i)));
    return map;
  }, [topicsList]);

  const activeTopics = useMemo(() => {
    const unmergedTopicIds = new Set<string>();
    for (const c of changes) {
      if (!c.merged && c.topic_id) unmergedTopicIds.add(c.topic_id);
    }
    return topicsList
      .filter(t => t.status !== 'finished' && unmergedTopicIds.has(t.id))
      .sort((a, b) => a.created_at.localeCompare(b.created_at));
  }, [topicsList, changes]);

  const maxCols = useMemo(
    () => Math.max(1, ...graph.map(r => r.node_line.length)),
    [graph]
  );
  const graphWidth = maxCols * COL_WIDTH;

  if (changes.length === 0) {
    return <div className="p-3 text-gray-400 text-sm">No changes found</div>;
  }

  const mainChangeId = changes.find(c => c.merged)?.change_id;

  // Filter
  const filteredChanges = filterTopicId
    ? changes.filter(c => c.topic_id === filterTopicId || c.merged)
    : changes;

  const visibleIds = new Set(filteredChanges.map(c => c.change_id));
  const changeIndexMap = new Map<string, number>();
  changes.forEach((c, i) => changeIndexMap.set(c.change_id, i));

  const graphByChange = new Map<string, GraphRow>();
  const prevPadByChange = new Map<string, PadLine[]>();
  for (let gi = 0; gi < graph.length; gi++) {
    graphByChange.set(graph[gi].node, graph[gi]);
    if (gi > 0) prevPadByChange.set(graph[gi].node, graph[gi - 1].pad_lines);
  }

  const items: DisplayItem[] = [];

  for (let i = 0; i < filteredChanges.length; i++) {
    const change = filteredChanges[i];
    if (filterTopicId && i > 0) {
      const prevIdx = changeIndexMap.get(filteredChanges[i - 1].change_id)!;
      const currIdx = changeIndexMap.get(change.change_id)!;
      let hiddenCount = 0;
      for (let j = prevIdx + 1; j < currIdx; j++) {
        if (!visibleIds.has(changes[j].change_id)) hiddenCount++;
      }
      if (hiddenCount > 0) items.push({ type: 'gap', count: hiddenCount });
    }
    items.push({ type: 'change', change, graphRow: graphByChange.get(change.change_id), prevPadLines: prevPadByChange.get(change.change_id) });
  }

  return (
    <div className="overflow-hidden">
      {/* Topic filter chips */}
      {activeTopics.length > 0 && (
        <div className="px-2 py-1.5 border-b border-gray-200 flex flex-wrap gap-1">
          <button
            onClick={() => setFilterTopicId(null)}
            className={`text-xs px-2 py-0.5 rounded-full transition-colors ${
              filterTopicId === null
                ? 'bg-gray-700 text-white'
                : 'bg-gray-200 text-gray-600 hover:bg-gray-300'
            }`}
          >
            All
          </button>
          {activeTopics.map(topic => {
            const color = topicColorMap.get(topic.id);
            const isActive = filterTopicId === topic.id;
            return (
              <button
                key={topic.id}
                onClick={() => setFilterTopicId(isActive ? null : topic.id)}
                className="text-xs px-2 py-0.5 rounded-full transition-colors"
                style={{
                  backgroundColor: isActive ? color : undefined,
                  color: isActive ? 'white' : color,
                  border: `1px solid ${color}`,
                }}
              >
                {topic.id}
              </button>
            );
          })}
        </div>
      )}

      {/* Flat DAG list with divide-y; graph container uses negative margin to bridge borders */}
      <div className="divide-y divide-gray-200">
        {items.map((item, idx) => {
          if (item.type === 'gap') {
            return (
              <div
                key={`gap-${idx}`}
                className="flex items-center text-xs text-gray-400 italic py-1"
              >
                <div style={{ width: graphWidth }} className="flex-shrink-0" />
                <div className="px-2">{item.count} hidden</div>
              </div>
            );
          }
          const { change, graphRow, prevPadLines } = item;
          const isSelected = selectedChangeId === change.change_id;
          const isMain = change.change_id === mainChangeId;
          const topic = change.topic_id ? topicById.get(change.topic_id) : undefined;
          const topicColor = change.topic_id ? topicColorMap.get(change.topic_id) : undefined;
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
                  topicSlug={topic?.id}
                  topicColor={topicColor}
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
