import type { NotificationInput } from '../notification-center-store';
import { userMessage } from '../agent-session/user-message';

/** When Core's answer has no sentence the user can read. */
export const MODEL_FAILURE_FALLBACK = '操作没有完成。请刷新模型设置，核对当前状态后重试。';

/** The notifications the dialog raises for failures and deletions. */
export const notifyFailure = (
  notify: (notification: NotificationInput) => void,
  title: string,
  key: string,
  error: { code: string; message: string },
) => {
  notify({
    dedupeKey: `${key}:${error.code}`,
    message: userMessage(error, MODEL_FAILURE_FALLBACK),
    title,
    tone: 'danger',
  });
};

/** Credential failures must name the failed layer and the next action; the
 * internal code stays in the dedupe key and the journal, not in the copy. */
export const credentialFailureMessages: Record<string, string> = {
  'credential.state_conflict': '这个凭据槽位的当前状态不允许该操作，请刷新模型设置后重试。',
  'credential.row_revision_conflict': '凭据状态已变化，请刷新模型设置后重新保存。',
  'credential.authentication_scheme_mismatch': '该 Endpoint 的认证方式与凭据操作不匹配。',
  'credential.reconciliation_required': '凭据需要先完成恢复处理，之后才能保存新的 API Key。',
  'credential.mutation_aborted': '凭据写入在提交前被中止，配置未改变，请重试。',
  'credential.store_unavailable': 'Windows 凭据存储拒绝了本次写入，API Key 未保存；请重试。',
  'models.configuration.revision_conflict': '模型配置已变化，请刷新后重新保存。',
  'models.endpoint.not_found': '该 Endpoint 已不存在，请刷新模型配置。',
};

export const notifyCredentialFailure = (
  notify: (notification: NotificationInput) => void,
  title: string,
  key: string,
  error: { code: string; message: string },
) => {
  notify({
    dedupeKey: `${key}:${error.code}`,
    message:
      credentialFailureMessages[error.code] ??
      'API Key 未保存，配置没有改变。请刷新模型设置后重试；诊断信息已保留在本机日志。',
    title,
    tone: 'danger',
  });
};

export const notifyDeletion = (
  notify: (notification: NotificationInput) => void,
  displayName: string,
  credentialCleanupPending: boolean,
) => {
  notify({
    dedupeKey: `model-config-deleted:${displayName}:${credentialCleanupPending}`,
    message: credentialCleanupPending
      ? '配置已删除，仍有凭据等待安全清理。配置不会重新进入 Runtime，也不能再解析该凭据。'
      : `${displayName} 已从本地模型配置删除。`,
    title: credentialCleanupPending ? '凭据清理待完成' : '配置已删除',
    tone: credentialCleanupPending ? 'warning' : 'success',
  });
};
