import { useRef, useEffect, useState, forwardRef } from 'react';
import { useAppContext } from '../context';
import { useChanges, useTopics, type Change } from '../hooks';
import type { Topic } from '../types';

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
      className={`w-full text-left px-3 py-2 transition-colors ${
        isSelected && focused
          ? 'bg-blue-100 hover:bg-blue-100 border-l-2 border-blue-500'
          : isSelected
            ? 'bg-blue-50 hover:bg-blue-100 border-l-2 border-blue-300'
            : change.merged
              ? 'hover:bg-gray-100 opacity-60'
              : 'hover:bg-gray-100'
      }`}
    >
      <div className="flex items-center gap-2">
        <span className="font-mono text-xs text-gray-400">{change.change_id.slice(0, 8)}</span>
        {isMain && (
          <span className="text-xs bg-green-100 text-green-700 px-1.5 py-0.5 rounded font-medium">
            main
          </span>
        )}
        {change.empty && (
          <span className="text-xs bg-gray-200 text-gray-500 px-1.5 py-0.5 rounded">
            empty
          </span>
        )}
        <span className="flex-1" />
        {change.open_thread_count > 0 && (
          <span className="text-xs bg-amber-100 text-amber-700 px-1.5 py-0.5 rounded" title="Open threads">
            {change.open_thread_count} open
          </span>
        )}
      </div>
      <div className="text-sm truncate">
        {change.description || <span className="text-gray-400 italic">(no description)</span>}
      </div>
    </button>
  );
});

interface TopicHeaderProps {
  topic: Topic;
  expanded: boolean;
  onToggle: () => void;
  totalOpenThreads: number;
}

function TopicHeader({ topic, expanded, onToggle, totalOpenThreads }: TopicHeaderProps) {
  return (
    <button
      onClick={onToggle}
      className="w-full text-left px-3 py-1.5 bg-gray-50 hover:bg-gray-100 border-b border-gray-200 flex items-center gap-2"
    >
      <span className="text-xs text-gray-400">{expanded ? '\u25BC' : '\u25B6'}</span>
      <span className="text-sm font-medium text-gray-700 truncate flex-1">{topic.name}</span>
      <span className="text-xs text-gray-400">
        {topic.change_count} {topic.change_count === 1 ? 'change' : 'changes'}
      </span>
      {totalOpenThreads > 0 && (
        <span className="text-xs bg-amber-100 text-amber-700 px-1.5 py-0.5 rounded">
          {totalOpenThreads} open
        </span>
      )}
    </button>
  );
}

export function ChangeList() {
  const changes = useChanges();
  const { topics: topicsList } = useTopics();

  const { focusedPanel, selectedChangeId, selectChange } = useAppContext();
  const focused = focusedPanel === 'changes';

  // Non-finished topics start expanded
  const [expandedTopics, setExpandedTopics] = useState<Set<string>>(() => {
    return new Set(topicsList.filter(t => t.status !== 'finished').map(t => t.id));
  });

  const changeRefs = useRef<Map<string, HTMLButtonElement>>(new Map());

  useEffect(() => {
    if (selectedChangeId && focused) {
      const el = changeRefs.current.get(selectedChangeId);
      el?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  }, [selectedChangeId, focused]);

  if (changes.length === 0) {
    return <div className="p-3 text-gray-400 text-sm">No changes found</div>;
  }

  const mainChangeIdx = changes.findIndex((c) => c.merged);

  const toggleTopic = (topicId: string) => {
    setExpandedTopics(prev => {
      const next = new Set(prev);
      if (next.has(topicId)) next.delete(topicId);
      else next.add(topicId);
      return next;
    });
  };

  // Build topic-to-changes map from the changes list (which has topological order from the API)
  const topicChanges = new Map<string, Change[]>();
  const unassignedChanges: Change[] = [];
  const mergedChanges: Change[] = [];

  for (const change of changes) {
    if (change.merged) {
      mergedChanges.push(change);
    } else if (change.topic_id) {
      const list = topicChanges.get(change.topic_id) || [];
      list.push(change);
      topicChanges.set(change.topic_id, list);
    } else {
      unassignedChanges.push(change);
    }
  }

  // Order topics by creation date
  const sortedTopics = [...topicsList].sort((a, b) => {
    return new Date(a.created_at).getTime() - new Date(b.created_at).getTime();
  });

  // Only show topics that have unmerged changes
  const visibleTopics = sortedTopics.filter(t => {
    const tc = topicChanges.get(t.id);
    return tc && tc.length > 0;
  });

  const hasTopics = visibleTopics.length > 0;

  return (
    <div className="divide-y divide-gray-200">
      {/* Topic sections */}
      {visibleTopics.map(topic => {
        const tc = topicChanges.get(topic.id) || [];
        const expanded = expandedTopics.has(topic.id);
        const totalOpen = tc.reduce((sum, c) => sum + c.open_thread_count, 0);

        return (
          <div key={topic.id}>
            <TopicHeader
              topic={topic}
              expanded={expanded}
              onToggle={() => toggleTopic(topic.id)}
              totalOpenThreads={totalOpen}
            />
            {expanded && (
              <div className="divide-y divide-gray-200">
                {tc.map(change => {
                  const isSelected = selectedChangeId === change.change_id;
                  const isMain = changes.indexOf(change) === mainChangeIdx;
                  return (
                    <ChangeItem
                      key={change.change_id}
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
                  );
                })}
              </div>
            )}
          </div>
        );
      })}

      {/* Unassigned changes */}
      {unassignedChanges.length > 0 && hasTopics && (
        <div className="px-3 py-1.5 bg-gray-50 border-b border-gray-200 text-sm font-medium text-gray-700">
          Other changes
        </div>
      )}
      {unassignedChanges.map(change => {
        const isSelected = selectedChangeId === change.change_id;
        const isMain = changes.indexOf(change) === mainChangeIdx;
        return (
          <ChangeItem
            key={change.change_id}
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
        );
      })}

      {/* Merged changes */}
      {mergedChanges.length > 0 && (
        <div className="px-3 py-1.5 bg-gray-50 border-b border-gray-200 text-sm font-medium text-gray-700">
          Merged
        </div>
      )}
      {mergedChanges.map((change, idx) => {
        const isSelected = selectedChangeId === change.change_id;
        const isMain = idx === 0; // First merged change is where main points
        return (
          <ChangeItem
            key={change.change_id}
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
        );
      })}
    </div>
  );
}
