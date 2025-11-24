import { Change } from '../api';

interface Props {
  changes: Change[];
  selectedId?: string;
  onSelect: (change: Change) => void;
  loading: boolean;
}

export function ChangeList({ changes, selectedId, onSelect, loading }: Props) {
  if (loading) {
    return (
      <div className="p-4 text-gray-500">Loading changes...</div>
    );
  }

  if (changes.length === 0) {
    return (
      <div className="p-4 text-gray-500">No changes found</div>
    );
  }

  return (
    <div className="divide-y divide-gray-700">
      {changes.map((change) => (
        <button
          key={change.change_id}
          onClick={() => onSelect(change)}
          className={`w-full text-left p-4 hover:bg-gray-800 transition-colors ${
            selectedId === change.change_id ? 'bg-gray-800' : ''
          }`}
        >
          <div className="font-mono text-xs text-gray-500">
            {change.change_id.slice(0, 12)}
          </div>
          <div className="mt-1 text-sm truncate">
            {change.description || <span className="text-gray-500 italic">(no description)</span>}
          </div>
          <div className="mt-1 text-xs text-gray-500">
            {change.author}
          </div>
          {change.empty && (
            <span className="inline-block mt-1 text-xs bg-gray-700 text-gray-400 px-2 py-0.5 rounded">
              empty
            </span>
          )}
        </button>
      ))}
    </div>
  );
}
