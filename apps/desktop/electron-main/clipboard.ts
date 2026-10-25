import { clipboard } from 'electron';
import type { AgentMainParams, AgentMainResult } from '../shared/generated/routes.js';

type Request = AgentMainParams['agent.clipboard.write'];
type Written = AgentMainResult['agent.clipboard.write'];

/** The most the clipboard is given at once, as UTF-8. */
const MOST_BYTES = 16 * 1024 * 1024;

/**
 * Main refuses
 * the browser's clipboard permissions to every page, so text the user copies — a reply, a
 * message, a path, a link — is written here. It is only ever text, and never more than 16 MiB.
 */
export function writeClipboard(request: Request): Written {
  if (Buffer.byteLength(request.text, 'utf8') > MOST_BYTES)
    return { completed: false, message: '内容太大（超过 16 MB），没有复制。' };
  try {
    clipboard.writeText(request.text);
  } catch {
    return { completed: false, message: '剪贴板暂时不可用（可能正被其他程序占用），请重试。' };
  }
  return { completed: true, message: '已复制' };
}
