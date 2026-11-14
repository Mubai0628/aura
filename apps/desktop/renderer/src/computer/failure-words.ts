import type { ComputerControlledV1 } from '@aura/contracts/generated';

import {
  isUnconfirmedCommandOutcome,
  type IpcFailureLike,
} from '../../../shared/ipc-request-failure';
import { connectionCause } from '../agent-session/failure-text';
import { userMessage } from '../agent-session/user-message';

/**
 * What a stop or a give-back that did not take effect says: why, what it means for the desktop,
 * and what to do; and what a desktop given back whose message to the model was not sent says.
 */

const ended = (sentence: string): string =>
  /[。！？.!?]$/u.test(sentence) ? sentence : `${sentence}。`;

/**
 * `what` did not happen, with Core's own sentence after it when it has one, else the connection
 * as the cause; the next clause follows.
 */
function opening(what: string, error: unknown): string {
  const cause = error === undefined ? undefined : connectionCause(error);
  const said = error === undefined || cause !== undefined ? '' : userMessage(error, '');
  if (said) return `${what}：${ended(said)}`;
  return `${what}${cause ? `（${cause}）` : ''}，`;
}

/** A stop of one program (`app`), or of every one, that did not take effect. */
export function stopFailure(
  error: unknown,
  apps: readonly ComputerControlledV1[],
  app?: string,
): string {
  const targets = app === undefined ? apps : apps.filter((one) => one.app_id === app);
  const named = targets.length === 1 ? `「${targets[0]!.name}」` : '这些程序';
  // Esc takes the desktop over from a program the model drives on the user's own desktop.
  const esc = targets.some((one) => one.mode !== 'child_session') ? '，需要时按 Esc 接管' : '';
  return `${opening('没有停止', error)}模型可能仍在操控${named}；请再点一次“${app === undefined ? '全部停止' : '停止'}”${esc}。`;
}

/** A give-back that did not take effect: the desktop is still the user's. */
export const giveBackFailure = (error: unknown, app: ComputerControlledV1): string =>
  `${opening('没有交还', error)}「${app.name}」仍由你操作。请再点一次“交还”。`;

const UNCONFIRMED_NEXT = '请看对话里有没有这条消息，没有就在输入框发送“继续”。';

/**
 * The desktop was given back but the message telling the model to go on failed: without it the
 * model does not go on by itself. `thrown`: no answer came at all, so whether it was sent is open.
 */
export function toldModelFailure(error: unknown, thrown = false): string {
  const cause = connectionCause(error);
  const unconfirmed =
    typeof error === 'object' &&
    error !== null &&
    isUnconfirmedCommandOutcome(error as IpcFailureLike);
  if (thrown || unconfirmed)
    return `操作权已交还，但告诉模型继续的消息是否发出没有确认${cause ? `（${cause}）` : ''}；${UNCONFIRMED_NEXT}`;
  const said = cause === undefined ? userMessage(error, '') : '';
  const why = said ? `：${said.replace(/[。.]+$/u, '')}。` : `${cause ? `（${cause}）` : ''}。`;
  return `操作权已交还，但告诉模型继续的消息没有发出${why}请在输入框发送“继续”，模型才会接着做。`;
}
