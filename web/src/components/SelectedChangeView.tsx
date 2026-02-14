import { useRef, useMemo } from 'react';
import { DiffViewer, DiffViewerHandle } from './DiffViewer';
import { CommentPanel } from './CommentPanel';
import { useAppContext } from '../context';
import { useDiff, useReview, useChanges, type Change } from '../hooks';

interface SelectedChangeViewProps {
  change: Change;
}

export function SelectedChangeView({ change }: SelectedChangeViewProps) {
  const { setFocusedPanel, selectedSessionName, selectedSessionVersion, interdiffActive } = useAppContext();

  const diffViewerRef = useRef<DiffViewerHandle>(null);

  // Look up the base commit for interdiff mode
  const { sessions } = useChanges();
  const interdiffBaseCommitId = useMemo(() => {
    if (!interdiffActive || !selectedSessionName) return undefined;
    const session = sessions.find(s => s.name === selectedSessionName);
    if (!session) return undefined;
    // Find the base push: for live → latest push, for push i → push i+1 (older)
    const reversedPushes = [...session.pushes].reverse();
    let basePush;
    if (selectedSessionVersion === 'live') {
      basePush = reversedPushes[0];
    } else {
      const idx = parseInt(selectedSessionVersion, 10);
      basePush = isNaN(idx) ? undefined : reversedPushes[idx + 1];
    }
    if (!basePush) return undefined;
    // Find this change's commit in the base push
    const match = basePush.changes.find(c => c.change_id === change.change_id);
    return match?.commit_id;
  }, [interdiffActive, selectedSessionName, selectedSessionVersion, sessions, change.change_id]);

  // Fetch data - these suspend until ready
  // Pass session name so backend queries the clone for live session changes
  const review = useReview(change.change_id);
  const diffResponse = useDiff(
    change.change_id,
    change.commit_id,
    interdiffBaseCommitId,
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
