import {
  isUnconfirmedCommandOutcome,
  type IpcFailureLike,
} from '../../../shared/ipc-request-failure';
import { connectionCause, failedAction } from '../agent-session/failure-text';
import { userMessage } from '../agent-session/user-message';

/**
 * What a terminal or a process that failed says: why when known, what it means for the terminal
 * or the program running in it, and what to do. A failure of something the user did not start
 * (following output, following the panel's size) does not ask to retry it.
 */

const bracket = (cause: string | undefined): string => (cause ? `（${cause}）` : '');

const unconfirmed = (error: unknown): boolean =>
  typeof error === 'object' &&
  error !== null &&
  isUnconfirmedCommandOutcome(error as IpcFailureLike);

/** Typed input that did not reach the terminal, or whose arrival was not confirmed. */
export function inputFailure(error: unknown): string {
  const cause = connectionCause(error);
  if (unconfirmed(error))
    return `这次输入是否送到终端没有确认${bracket(cause)}；请看终端当前内容再决定是否重新输入。`;
  return userMessage(
    error,
    `这次输入没有送到终端${bracket(cause)}，终端里可能缺了这段输入。请看终端当前内容再决定是否重新输入。`,
  );
}

/** A size the terminal did not take; the next change of the panel's size sends it again. */
export const resizeFailure = (error: unknown): string =>
  failedAction(error, '终端没能跟着面板调整大小', '显示可能错位。再拖动一次面板宽度会重新调整。');

/** The terminal's output could not be subscribed to. */
export const subscribeFailure = (error: unknown): string =>
  failedAction(error, '没有接上这个终端的输出', '屏幕可能不再更新。请关闭这个页签后重新打开终端。');

/** Output that did not line up with what the screen already shows. */
export const OUTPUT_OUT_OF_STEP =
  '终端输出没有对上，屏幕已停止更新。请关闭这个页签后重新打开终端。';

/** A tab whose terminal Core no longer has. */
export const TERMINAL_GONE =
  '这个终端已经结束（例如 Aura Core 重启过），无法再连上。请关闭这个页签，需要时新开一个终端。';

/** The conversation's terminals could not be listed; opening the conversation again reads them. */
export const listFailure = (error: unknown): string =>
  failedAction(
    error,
    '终端列表读不出来',
    '之前打开的终端暂时没有列出。切换到别的会话再切回来会重新读取。',
  );

/** A close that did not take effect: the terminal and its program still run. */
export const closeFailure = (error: unknown): string =>
  failedAction(error, '终端没有关闭', '它仍在运行，页签已保留；请再关一次。');

/** Core's codes for a secure input request that has already ended. */
const SENSITIVE_ENDED = [
  'terminal.sensitive.not_pending',
  'terminal.sensitive.expired_or_consumed',
  'terminal.sensitive.consumed',
];

const field = (value: unknown, name: 'code' | 'message'): string => {
  if (typeof value !== 'object' || value === null) return '';
  const found = (value as Record<string, unknown>)[name];
  return typeof found === 'string' ? found : '';
};

/** Secure input that was refused; nothing reached the terminal. */
export function sensitiveFailure(error: unknown): string {
  const code = field(error, 'code');
  const message = field(error, 'message');
  if (SENSITIVE_ENDED.some((key) => code === key || message.startsWith(key)))
    return '这次安全输入请求已经结束（超时或已经用过），没有输入任何内容。请查看终端，需要时让模型再请求一次。';
  return failedAction(error, '安全输入没有完成', '没有输入任何内容；请重新输入后再提交，或取消。');
}

/** Secure input whose answer never came: it may have been typed. */
export const SENSITIVE_UNCONFIRMED =
  '没能确认这次输入是否送到终端。请先看终端当前状态，确认没收到再输入一次，避免重复输入。';

/** The model's background processes could not be listed. */
export const processListFailure = (error: unknown): string =>
  failedAction(error, '进程列表读不出来', '点“刷新”再试。');

/** A stop that did not take effect: the program may still run. `refresh` names the button. */
export const processStopFailure = (error: unknown, refresh = '刷新'): string =>
  failedAction(error, '停止没有完成', `进程可能仍在运行；点“${refresh}”查看，仍在运行就再停一次。`);
