import { useRef, useEffect, forwardRef } from 'react';
import { useAppStore } from '../store';
import type { Change } from '../api';

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
        <span className="font-mono text-xs text-gray-400">{change.change_id.slice(0, 12)}</span>
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
        {change.revision_count > 0 && (
          <span className="text-xs bg-gray-100 text-gray-600 px-1.5 py-0.5 rounded font-mono" title="Revision count">
            v{change.revision_count}
          </span>
        )}
        {change.has_pending_changes && (
          <span className="text-xs bg-blue-100 text-blue-700 px-1.5 py-0.5 rounded" title="Has uncommitted changes since last revision">
            pending
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

export function ChangeList() {
  const changes = useAppStore((s) => s.changes);
  const selectedChange = useAppStore((s) => s.selectedChange);
  const loading = useAppStore((s) => s.loading);
  const focused = useAppStore((s) => s.focusedPanel === 'changes');
  const selectChange = useAppStore((s) => s.selectChange);

  const changeRefs = useRef<Map<string, HTMLButtonElement>>(new Map());

  // Scroll selected change into view when it changes
  useEffect(() => {
    if (selectedChange && focused) {
      const el = changeRefs.current.get(selectedChange.change_id);
      el?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  }, [selectedChange?.change_id, focused]);

  if (loading && changes.length === 0) {
    return <div className="p-3 text-gray-400 text-sm">Loading changes...</div>;
  }

  if (changes.length === 0) {
    return <div className="p-3 text-gray-400 text-sm">No changes found</div>;
  }

  // Find the change that main points to (first merged change)
  const mainChangeIdx = changes.findIndex((c) => c.merged);

  return (
    <div className="divide-y divide-gray-200">
      {changes.map((change, idx) => {
        const isSelected = selectedChange?.change_id === change.change_id;
        const isMain = idx === mainChangeIdx;

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
            onClick={() => selectChange(change)}
          />
        );
      })}
    </div>
  );
}
