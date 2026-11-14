import type { NotificationInput } from '../notification-center-store';
import { userMessage } from '../agent-session/user-message';
import { FACTS_CHANGED, modelFailureText } from './model-failure-words';

type Notify = (notification: NotificationInput) => void;

/** The notifications the dialog raises for failures and deletions. */
export const notifyFailure = (
  notify: (notification: NotificationInput) => void,
  title: string,
  key: string,
  error: { code: string; message: string },
) => {
  notify({
    dedupeKey: `${key}:${error.code}`,
    message: modelFailureText(error),
    title,
    tone: 'danger',
  });
};

/** A key that was not saved or cleared, in `message`; the code stays in the dedupe key. */
export const notifyCredentialFailure = (
  notify: (notification: NotificationInput) => void,
  title: string,
  key: string,
  code: string,
  message: string,
) => {
  notify({ dedupeKey: `${key}:${code}`, message, title, tone: 'danger' });
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
          `${displayName} 已删除，仍有凭据等待安全清理，Aura 会在后台继续重试。这份配置不会再被使用，这个凭据也不会再被读取。`,
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
          ? modelFailureText(error)
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
