import { useAppStore } from '../store';
import type { Change } from '../api';

function ChangeItem({
  change,
  isSelected,
  focused,
  isMain,
  onClick,
}: {
  change: Change;
  isSelected: boolean;
  focused: boolean;
  isMain: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`w-full text-left p-4 transition-colors ${
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
      </div>
      <div className="mt-1 text-sm truncate">
        {change.description || <span className="text-gray-400 italic">(no description)</span>}
      </div>
      <div className="mt-1 text-xs text-gray-400">{change.author}</div>
    </button>
  );
}

export function ChangeList() {
  const changes = useAppStore((s) => s.changes);
  const selectedChange = useAppStore((s) => s.selectedChange);
  const loading = useAppStore((s) => s.loading);
  const focused = useAppStore((s) => s.focusedPanel === 'changes');
  const selectChange = useAppStore((s) => s.selectChange);

  if (loading && changes.length === 0) {
    return <div className="p-4 text-gray-400">Loading changes...</div>;
  }

  if (changes.length === 0) {
    return <div className="p-4 text-gray-400">No changes found</div>;
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
