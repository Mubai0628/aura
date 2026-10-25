import { ConfirmationDialog } from './ConfirmationDialog';
import type { useSecurityControl } from './security-control-store';
import { FULL_ACCESS_DELETES } from './security/full-access-notice';

export function PermissionModeFeedback({
  security,
}: {
  security: ReturnType<typeof useSecurityControl>;
}) {
  if (!security.fullAccessConfirmation) return null;
  return (
    <ConfirmationDialog
      busy={security.modeChanging}
      confirmLabel="启用完全访问"
      description={`模型可使用工具联网、上传文件，并在已授权范围内修改或删除文件。请仅对你信任的任务启用；完全访问不会自动授予管理员权限。${FULL_ACCESS_DELETES}`}
      onCancel={security.cancelFullAccess}
      onConfirm={() => void security.confirmFullAccess()}
      title="启用本会话的完全访问"
      titleId="full-access-confirmation-title"
    />
  );
}
