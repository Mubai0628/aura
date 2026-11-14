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
import { envelopeOf } from './request-failure-envelopes.js';
const CORE_ABSENT = new Error('Core unavailable');

export function registerTerminalOpeningIpc(
  getCore: () => CoreSupervisor | undefined,
  trust: (event: IpcMainInvokeEvent) => void,
) {
  const openings = new TerminalOpenings((owner, change) => {
    const contents = owner as WebContents;
    if (!contents.isDestroyed()) contents.send(TERMINAL_OPENING_EVENT_CHANNEL, change);
  });
  const watched = new WeakSet<WebContents>();
  // Without Core, or without a channel to it, nothing was asked: Core is starting or restarting.
  const failure = (error: unknown) => ({
    ok: false as const,
    error:
      error instanceof CoreRpcError
        ? error.envelope
        : errorEnvelope(
            'terminal.opening.unavailable',
            error === CORE_ABSENT || envelopeOf(error)?.code === 'ipc.request_not_sent'
              ? 'Aura Core 正在启动或重启，终端没有打开；等 Core 就绪后再点一次。'
              : '终端没有打开（Aura 内部出错）；请再点一次，仍不行请重启 Aura。',
            'failed',
          ),
  });
  ipcMain.handle(TERMINAL_OPENING_START_CHANNEL, async (event, ...args: unknown[]) => {
    try {
      trust(event);
      if (args.length !== 1 || !isTerminalOpeningStart(args[0]))
        throw new Error('invalid opening request');
      const core = getCore();
      if (!core) throw CORE_ABSENT;
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
