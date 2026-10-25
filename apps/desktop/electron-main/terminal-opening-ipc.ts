import { ipcMain, type IpcMainInvokeEvent, type WebContents } from 'electron';
import { CoreRpcError, errorEnvelope } from './core-transport.js';
import type { CoreSupervisor } from './core-supervisor.js';
import { TerminalOpenings } from './terminal-opening.js';
import {
  isTerminalOpeningStart,
  isTerminalOpeningRef,
} from '../shared/terminal-opening-validation.js';
import {
  TERMINAL_OPENING_START_CHANNEL,
  TERMINAL_OPENING_CANCEL_CHANNEL,
  TERMINAL_OPENING_EVENT_CHANNEL,
} from '../shared/terminal-opening-api.js';
export function registerTerminalOpeningIpc(
  getCore: () => CoreSupervisor | undefined,
  trust: (event: IpcMainInvokeEvent) => void,
) {
  const openings = new TerminalOpenings((owner, change) => {
    const contents = owner as WebContents;
    if (!contents.isDestroyed()) contents.send(TERMINAL_OPENING_EVENT_CHANNEL, change);
  });
  const watched = new WeakSet<WebContents>();
  const failure = (error: unknown) => ({
    ok: false as const,
    error:
      error instanceof CoreRpcError
        ? error.envelope
        : errorEnvelope(
            'terminal.opening.unavailable',
            '终端打开状态暂不可用，请检查 Core 状态。',
            'failed',
          ),
  });
  ipcMain.handle(TERMINAL_OPENING_START_CHANNEL, async (event, ...args: unknown[]) => {
    try {
      trust(event);
      if (args.length !== 1 || !isTerminalOpeningStart(args[0]))
        throw new Error('invalid opening request');
      const core = getCore();
      if (!core) throw new Error('Core unavailable');
      if (!watched.has(event.sender)) {
        watched.add(event.sender);
        event.sender.once('destroyed', () => openings.closeOwner(event.sender));
      }
      return { ok: true, value: await openings.start(event.sender, args[0], core) };
    } catch (error) {
      return failure(error);
    }
  });
  ipcMain.handle(TERMINAL_OPENING_CANCEL_CHANNEL, async (event, ...args: unknown[]) => {
    try {
      trust(event);
      if (args.length !== 1 || !isTerminalOpeningRef(args[0]))
        throw new Error('invalid opening reference');
      return { ok: true, value: await openings.cancel(event.sender, args[0].opening_id) };
    } catch (error) {
      return failure(error);
    }
  });
}
