import { useRef } from 'react';
import { DiffViewer, DiffViewerHandle } from './DiffViewer';
import { CommentPanel } from './CommentPanel';
import { useAppContext } from '../context';
import { useDiff, useReview, type Change } from '../hooks';

interface SelectedChangeViewProps {
  change: Change;
}

export function SelectedChangeView({ change }: SelectedChangeViewProps) {
  const { selectedRevision, comparisonBase, setFocusedPanel } = useAppContext();

  const diffViewerRef = useRef<DiffViewerHandle>(null);

  // Fetch data - these suspend until ready
  // Note: both fetches happen in parallel since neither depends on the other
  // When selectedRevision is null, backend returns latest diff
  const review = useReview(change.change_id);
  const diffResponse = useDiff(
    change.change_id,
    selectedRevision?.commit_id,
    comparisonBase?.commit_id
  );

  return (
    <div className="flex-1 flex overflow-hidden">
      <div
        className="flex-1 overflow-auto"
        onClick={() => setFocusedPanel('diff')}
      >
        <DiffViewer
          ref={diffViewerRef}
          diff={diffResponse.diff}
          targetMessage={diffResponse.target_message}
          messageDiff={diffResponse.message_diff}
          review={review}
          changeId={change.change_id}
          description={change.description}
        />
      </div>

      <aside
        className="w-96 border-l border-gray-200 overflow-y-auto bg-gray-50"
        onClick={() => setFocusedPanel('threads')}
      >
        <CommentPanel
          review={review}
          selectedChange={change}
        />
      </aside>
    </div>
  );
}
