import type { WorkspaceNavigationItem } from '../app-shell-types';
import type { NotificationInput } from '../notification-center-store';
import { RelocateQuestionDialog } from './RelocateQuestionDialog';
import { useWorkspaceRelocation } from './useWorkspaceRelocation';
import './workspace-relocate.css';

/** On the row of a Workspace Core could not open at its folder: pick where the folder is now. */
export function WorkspaceRelocateButton({
  workspace,
  onNotify,
}: {
  workspace: WorkspaceNavigationItem;
  onNotify: (notice: NotificationInput) => void;
}) {
  const relocation = useWorkspaceRelocation(workspace, onNotify);
  return (
    <>
      <button
        type="button"
        className="button is-small workspace-relocate-button"
        aria-label={`为 ${workspace.name} 选择新位置`}
        aria-busy={relocation.busy ? true : undefined}
        disabled={relocation.busy}
        onClick={() => void relocation.start()}
      >
        选择新位置
      </button>
      {relocation.question && (
        <RelocateQuestionDialog
          busy={relocation.busy}
          error={relocation.questionError}
          question={relocation.question}
          onCancel={relocation.cancel}
          onConfirm={() => void relocation.confirm()}
        />
      )}
    </>
  );
}
