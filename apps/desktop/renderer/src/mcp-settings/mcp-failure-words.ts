import { answerMissing, connectionCause, refusedAction } from '../agent-session/failure-text';

/**
 * What the MCP and plugin secret forms say when a secret or a check did not go through: what was
 * and was not saved, why, and what to do next.
 */

const codeOf = (error: unknown): string => {
  if (typeof error !== 'object' || error === null) return '';
  const code = (error as { code?: unknown }).code;
  return typeof code === 'string' ? code : '';
};

/** Asks for a field left empty; a name in Latin letters is set apart by a space. */
export const fillFirst = (label: string): string =>
  `请先填写${/^[!-~]/u.test(label) ? ` ${label}` : label}。`;

/**
 * A typed secret (`label`) Core did not take while saving a server. The field was cleared when
 * the secret was read and the server configuration was not sent, so both have to be done again;
 * one whose outcome Main could not confirm may have been kept.
 */
export function secretNotSaved(label: string, failure: unknown): string {
  const code = codeOf(failure);
  const again = '输入框已清空，请重新填写后再保存。';
  if (code === 'ipc.command.outcome_unknown' || answerMissing(failure))
    return `${label}是否已保存没有确认，服务器设置没有保存；${again}`;
  const cause = code.endsWith('.secret_not_sent')
    ? 'Aura Core 正在重启或暂时连不上'
    : code.endsWith('.secret_invalid_input')
      ? '输入的内容无效'
      : connectionCause(failure);
  return `${label}没有保存${cause ? `（${cause}）` : ''}，服务器设置也没有保存；${again}`;
}

/** A configuration check that did not finish: why, and that nothing changed. */
export const checkFailure = (error: unknown): string =>
  refusedAction(
    error,
    '检查没有完成',
    connectionCause(error) === undefined
      ? '设置没有保存；修改后再检查。'
      : '设置没有保存；请稍后再检查。',
  );
