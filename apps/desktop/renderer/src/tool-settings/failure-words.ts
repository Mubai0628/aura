import { connectionCause, refusedAction } from '../agent-session/failure-text';
import { UNCONFIRMED_SAVE } from './useAgentConfiguration';

/**
 * What a tool-settings read or save that did not happen says: why, what is in force, and what to
 * do; a save whose outcome is not known says so as it is.
 */

const messageOf = (error: unknown): string => {
  if (typeof error === 'string') return error;
  if (typeof error !== 'object' || error === null) return '';
  const message = (error as { message?: unknown }).message;
  return typeof message === 'string' ? message : '';
};

/** Core's tool-settings refusals name their reason in the message. */
const CONFLICT = /revision_conflict/u;
const INVALID = /tools\.invalid_setting:/u;
const UNAVAILABLE = /tools\.configuration_unavailable/u;

/** `what` (a settings area) was not read: why, then `rest`: what that means and what to do. */
export function settingsNotRead(error: unknown, what: string, rest: string): string {
  const cause =
    connectionCause(error) ?? (UNAVAILABLE.test(messageOf(error)) ? 'Aura 内部出错' : undefined);
  return cause === undefined
    ? refusedAction(error, `${what}没有读到`, rest)
    : `${what}没有读到（${cause}），${rest}`;
}

/**
 * A save that failed (`what` did not happen): why, `impact` (what is in force) and `next`; one
 * refused because the settings changed elsewhere asks to read them again first. `field` names
 * the field Core found out of range. A save whose outcome is not known is said as it is.
 */
export function settingsNotSaved(
  error: string,
  what: string,
  impact: string,
  next: string,
  field?: string,
): string {
  if (error === UNCONFIRMED_SAVE) return error;
  if (CONFLICT.test(error))
    return `${what}（设置刚在别处被改过），${impact}。请重新打开这一页，核对后再改。`;
  if (INVALID.test(error))
    return `${what}（${field ? `「${field}」` : '有一项数值'}超出允许范围），${impact}。${next}。`;
  const cause = connectionCause(error);
  if (cause !== undefined) return `${what}（${cause}），${impact}。${next}。`;
  return refusedAction(error, what, `${impact}。${next}。`);
}

/** A proxy test Core refused or did not answer. */
export const probeFailure = (error: unknown): string =>
  refusedAction(error, '代理测试没有完成', '代理设置没有改变。请核对地址和端口后再测。');

/** A proxy test that never left Aura. */
export const PROBE_NOT_SENT =
  '代理测试没有发出去（Aura 内部通信失败），代理设置没有改变；请稍后再测。';

/**
 * A projection (`what`) not read yet: why, by the code its subscription failed with, and that it
 * is asked for again.
 */
export function projectionNotRead(what: string, code: string | undefined): string {
  const cause =
    code === 'bridge_rejected'
      ? 'Aura 内部通信失败'
      : connectionCause(code === undefined ? undefined : { code });
  return `${what}还没有读到${cause ? `（${cause}）` : ''}，正在重试；一直这样请重启 Aura。`;
}

/** Tool settings changed outside Aura that Core rejected: what is in force, and how to replace it. */
export const SETTINGS_FILE_REJECTED = {
  title: '工具设置文件里的改动无效',
  body: 'Aura 正在使用上次有效的设置；在本页改好后保存即可覆盖它。',
} as const;
