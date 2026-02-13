import { useRef } from 'react';
import { DiffViewer, DiffViewerHandle } from './DiffViewer';
import { CommentPanel } from './CommentPanel';
import { useAppContext } from '../context';
import { useDiff, useReview, type Change } from '../hooks';

interface SelectedChangeViewProps {
  change: Change;
}

export function SelectedChangeView({ change }: SelectedChangeViewProps) {
  const { setFocusedPanel, selectedSessionName } = useAppContext();

  const diffViewerRef = useRef<DiffViewerHandle>(null);

  // Fetch data - these suspend until ready
  // Pass session name so backend queries the clone for live session changes
  const review = useReview(change.change_id);
  const diffResponse = useDiff(
    change.change_id,
    change.commit_id,
    undefined,
    selectedSessionName ?? undefined
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
