/**
 * The model's side of the browser host — Core's commands, one at a time per conversation,
 * and their withdrawals; the page's DevTools connection; its fixed 1280x800 viewport drawn at the
 * sidebar's width; the user's own touch on the page while the model works, which takes the page
 * over; and the model's requests for the user's help, which the user answers.
 */
import type { WebContents } from 'electron';
import type { BrowserHostCommandV1, BrowserHostResultV1 } from '@aura/contracts/generated';
import { MODEL_PAGE_ID } from '../../shared/browser-api.js';
import { execute, VIEWPORT, type ExecutorDeps } from './actions.js';
import { Assists } from './assist.js';
import { PageDebugger, type DevTools } from './cdp.js';
import { PagePresses, keepKeyboardForUser } from './page-focus.js';
import { registerBrowserCommands } from './core-channel.js';
import type { BrowserPages, PageContents } from './pages.js';

/** How long after a model's command the user's own touch on its page takes the page over. */
const WORKING_MS = 60_000;
const USER_INPUT = new Set(['mouseDown', 'keyDown', 'rawKeyDown', 'touchStart']);

export interface CoreLink {
  requestWithBudget(method: string, params: unknown, timeoutMilliseconds: number): Promise<unknown>;
}

export interface ModelPages {
  /** Takes Core's commands from this supervisor's connection. */
  bind(owner: object): void;
  /** The user's answer to the model's request for help on this conversation's page. */
  answerAssist(sessionId: string, outcome: 'done' | 'declined'): boolean;
}

export function modelPages(
  pages: BrowserPages,
  core: () => CoreLink | undefined,
  log: (event: string, fields: Record<string, unknown>) => void,
  refreshSettings: () => Promise<void>,
  /** Gives the keyboard back to Aura's window. */
  giveBackFocus: () => void = () => undefined,
): ModelPages {
  const debuggers = new WeakMap<object, PageDebugger>();
  const lastCommand = new Map<string, number>();
  const queues = new Map<string, Promise<void>>();
  const assists = new Assists(pages);
  /** Commands waiting their turn, and those of them Core withdrew. */
  const queued = new Set<string>();
  const withdrawn = new Set<string>();

  const debuggerFor = (contents: PageContents): PageDebugger => {
    const known = debuggers.get(contents);
    if (known !== undefined) return known;
    const electron = contents as unknown as WebContents;
    const devtools = new PageDebugger(electron.debugger as unknown as DevTools);
    const presses = new PagePresses();
    keepKeyboardForUser(electron, presses, () => {
      if (pages.owner(electron.id)?.pageId === MODEL_PAGE_ID) giveBackFocus();
    });
    electron.on('input-event', (_event, input) => {
      if (devtools.synthetic > 0) return;
      if (input.type === 'mouseDown' || input.type === 'touchStart') presses.pressed();
      if (!USER_INPUT.has(input.type)) return;
      const owner = pages.owner(electron.id);
      if (owner?.pageId !== MODEL_PAGE_ID) return;
      // Asked for help, the user is meant to touch the page.
      if (pages.state(owner.sessionId, MODEL_PAGE_ID)?.assist !== undefined) return;
      if (Date.now() - (lastCommand.get(owner.sessionId) ?? 0) < WORKING_MS)
        pages.setUserControl(owner.sessionId, MODEL_PAGE_ID, true);
    });
    debuggers.set(contents, devtools);
    return devtools;
  };

  const emulate = async (sessionId: string, devtools: PageDebugger): Promise<number> => {
    const bounds = pages.bounds(sessionId, MODEL_PAGE_ID);
    const scale =
      bounds === undefined ? 0.5 : Math.min(1, Math.max(0.2, bounds.width / VIEWPORT.width));
    await devtools.send('Emulation.setDeviceMetricsOverride', {
      ...VIEWPORT,
      deviceScaleFactor: 0,
      mobile: false,
      scale,
    });
    // The page acts as if focused, so keys reach it while the user works elsewhere.
    await devtools.send('Emulation.setFocusEmulationEnabled', { enabled: true });
    return scale;
  };

  pages.onShown((sessionId, pageId) => {
    if (pageId !== MODEL_PAGE_ID) return;
    const contents = pages.contents(sessionId, pageId);
    const devtools = contents === undefined ? undefined : debuggers.get(contents);
    if (devtools !== undefined) void emulate(sessionId, devtools).catch(() => undefined);
  });

  const deps: ExecutorDeps = {
    pages,
    debuggerFor,
    emulate,
    assist: (sessionId, commandId, reason, timeoutMs) =>
      assists.wait(sessionId, commandId, reason, timeoutMs),
  };
  const answer = async (result: BrowserHostResultV1): Promise<void> => {
    try {
      await core()?.requestWithBudget('browser.host.result', result, 15_000);
    } catch (error) {
      log('browser.result_not_delivered', {
        command_id: result.command_id,
        reason: error instanceof Error ? error.message : String(error),
      });
    }
  };
  const run = (command: BrowserHostCommandV1): void => {
    const session = command.session_id;
    const id = command.command_id;
    queued.add(id);
    // One command at a time on a conversation's page, in the order Core sent them.
    const next = (queues.get(session) ?? Promise.resolve())
      .then(async () => {
        // The settings may not have been read by any user page yet, or may have changed since.
        await refreshSettings();
        queued.delete(id);
        // Core gave up on it before its turn came: it never runs.
        if (withdrawn.delete(id)) return;
        lastCommand.set(session, Date.now());
        const result = await execute(deps, command);
        lastCommand.set(session, Date.now());
        await answer(result);
      })
      .catch(() => undefined);
    queues.set(session, next);
    void next.then(() => {
      if (queues.get(session) === next) queues.delete(session);
    });
  };
  const withdraw = (commandId: string): void => {
    if (queued.has(commandId)) withdrawn.add(commandId);
    assists.withdraw(commandId);
  };
  return {
    bind: (owner) => registerBrowserCommands(owner, { run, withdraw }),
    answerAssist: (sessionId, outcome) => assists.answer(sessionId, outcome),
  };
}
