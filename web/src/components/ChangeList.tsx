import { Change } from '../api';

interface Props {
  changes: Change[];
  selectedId?: string;
  onSelect: (change: Change) => void;
  loading: boolean;
  focused: boolean;
}

export function ChangeList({ changes, selectedId, onSelect, loading, focused }: Props) {
  if (loading) {
    return (
      <div className="p-4 text-gray-400">Loading changes...</div>
    );
  }

  if (changes.length === 0) {
    return (
      <div className="p-4 text-gray-400">No changes found</div>
    );
  }

  return (
    <div className="divide-y divide-gray-200">
      {changes.map((change) => {
        const isSelected = selectedId === change.change_id;
        return (
        <button
          key={change.change_id}
          onClick={() => onSelect(change)}
          className={`w-full text-left p-4 hover:bg-gray-100 transition-colors ${
            isSelected && focused
              ? 'bg-blue-100 border-l-2 border-blue-500'
              : isSelected
                ? 'bg-blue-50 border-l-2 border-blue-300'
                : ''
          }`}
        >
          <div className="font-mono text-xs text-gray-400">
            {change.change_id.slice(0, 12)}
          </div>
          <div className="mt-1 text-sm truncate">
            {change.description || <span className="text-gray-400 italic">(no description)</span>}
          </div>
          <div className="mt-1 text-xs text-gray-400">
            {change.author}
          </div>
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
