/**
 * Core's browser commands arrive as the `browser.command.v1` notification on Main's
 * connection, and their withdrawals as `browser.cancel.v1`; the browser host registers per
 * supervisor what carries them out.
 */
import type { BrowserHostCommandV1 } from '@aura/contracts/generated';
import { readCancel, readCommand } from './protocol.js';

interface Handler {
  run(command: BrowserHostCommandV1): void;
  withdraw(commandId: string): void;
}
const handlers = new WeakMap<object, Handler>();

export function registerBrowserCommands(owner: object, handler: Handler): void {
  handlers.set(owner, handler);
}

/** True when the notification was a browser command (carried out, or dropped as malformed). */
export function browserCommandNotification(
  owner: object,
  method: string,
  params: unknown,
  runtimeId: string,
): boolean {
  if (method === 'browser.cancel.v1') {
    const cancel = readCancel(params, runtimeId);
    if (cancel !== undefined) handlers.get(owner)?.withdraw(cancel.command_id);
    return true;
  }
  if (method !== 'browser.command.v1') return false;
  const command = readCommand(params, runtimeId);
  // A malformed command is dropped: Core's own timer reports it unanswered.
  if (command !== undefined) handlers.get(owner)?.run(command);
  return true;
}
