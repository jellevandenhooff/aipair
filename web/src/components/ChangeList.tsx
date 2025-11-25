import { useAppStore } from '../store';

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

  return (
    <div className="divide-y divide-gray-200">
      {changes.map((change) => {
        const isSelected = selectedChange?.change_id === change.change_id;
        return (
          <button
            key={change.change_id}
            onClick={() => selectChange(change)}
            className={`w-full text-left p-4 transition-colors ${
              isSelected && focused
                ? 'bg-blue-100 hover:bg-blue-100 border-l-2 border-blue-500'
                : isSelected
                  ? 'bg-blue-50 hover:bg-blue-100 border-l-2 border-blue-300'
                  : 'hover:bg-gray-100'
            }`}
          >
            <div className="font-mono text-xs text-gray-400">
              {change.change_id.slice(0, 12)}
            </div>
            <div className="mt-1 text-sm truncate">
              {change.description || <span className="text-gray-400 italic">(no description)</span>}
            </div>
            <div className="mt-1 text-xs text-gray-400">{change.author}</div>
            {change.empty && (
              <span className="inline-block mt-1 text-xs bg-gray-200 text-gray-500 px-2 py-0.5 rounded">
                empty
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}
