import type { NotificationInput } from '../notification-center-store';
import { userMessage } from '../agent-session/user-message';
import { FACTS_CHANGED, MODEL_SETTINGS_CHANGING } from '../model-settings-store';

type Notify = (notification: NotificationInput) => void;

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
    message:
      error.code === FACTS_CHANGED
        ? MODEL_SETTINGS_CHANGING
        : userMessage(error, MODEL_FAILURE_FALLBACK),
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
  notify: Notify,
  displayName: string,
  credentialCleanupPending: boolean,
) => {
  notify(
    credentialCleanupPending
      ? credentialCleanupNotice(
          notify,
          `model-config-deleted:${displayName}:true`,
          `${displayName} 已删除，仍有凭据等待安全清理，Aura 会在后台继续重试。配置不会重新进入 Runtime，也不能再解析该凭据。`,
        )
      : {
          dedupeKey: `model-config-deleted:${displayName}:false`,
          message: `${displayName} 已从本地模型配置删除。`,
          title: '配置已删除',
          tone: 'success',
        },
  );
};

/** Credentials of deleted settings that Windows Credential Manager still holds, with 重试. */
const credentialCleanupNotice = (
  notify: Notify,
  dedupeKey: string,
  message: string,
): NotificationInput => ({
  action: retryAction(notify),
  dedupeKey,
  message,
  title: '凭据清理待完成',
  tone: 'warning',
});

const retryAction = (notify: Notify) => ({
  label: '重试',
  run: () => void retryCredentialCleanup(notify),
});

/** Tries every credential deleted settings left once more, and says what is left. */
export const retryCredentialCleanup = async (notify: Notify) => {
  const result = await window.aura.v1.retryModelCredentialCleanup({}).catch(() => undefined);
  if (result === undefined || !result.ok) {
    const error = result?.error;
    notify({
      action: retryAction(notify),
      dedupeKey: `model-credential-cleanup:${error?.code ?? 'unanswered'}`,
      message:
        error?.code === FACTS_CHANGED
          ? MODEL_SETTINGS_CHANGING
          : userMessage(error, '凭据清理没有完成，Aura 会在后台继续重试。'),
      title: '凭据清理未完成',
      tone: 'danger',
    });
    return;
  }
  const pending = result.value.pending_credentials;
  notify(
    pending === 0
      ? {
          dedupeKey: 'model-credential-cleanup:done',
          message: '已删除配置的凭据都已从 Windows 凭据管理器清理。',
          title: '凭据已清理',
          tone: 'success',
        }
      : credentialCleanupNotice(
          notify,
          'model-credential-cleanup:pending',
          `Windows 凭据管理器仍未删除 ${pending} 个凭据，Aura 会在后台继续重试。`,
        ),
  );
};
