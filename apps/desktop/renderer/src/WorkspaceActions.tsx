import { AppIcon } from './app-icons';
import type { NotificationInput } from './notification-center-store';
import { useAddWorkspace } from './workspace-add';

/** The sidebar's `+`: its outcome is one of the app's notices, never a popover over the list. */
export function WorkspaceActions({
  onCreated,
  onNotify,
}: {
  onCreated: (workspace: string) => void;
  onNotify: (notice: NotificationInput) => void;
}) {
  const { add, busy } = useAddWorkspace(onCreated, onNotify);
  return (
    <div className="sidebar-add-workspace">
      <button
        type="button"
        className="icon-button"
        aria-label="添加工作区"
        title="添加工作区"
        disabled={busy}
        onClick={() => void add()}
      >
        <AppIcon name={busy ? 'refresh' : 'plus'} size={17} />
      </button>
    </div>
  );
}
