import { ipcRenderer, type IpcRendererEvent } from 'electron';
import type {
  AgentMainCommand,
  AgentMainParams,
  AgentMainResult,
} from '../shared/generated/routes.js';
import {
  BROWSER_COMMAND_CHANNEL,
  BROWSER_PAGES_CHANNEL,
  BROWSER_STATE_CHANNEL,
  type BrowserApiV1,
  type BrowserCommandResult,
  type BrowserPageState,
} from '../shared/browser-api.js';
import type { ResultEnvelope } from '../shared/aura-api.js';

const NOTICES = new Set(['blocked', 'download', 'popup', 'failed', 'crashed', 'closed']);
const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

/** A page state from Main, in exactly its shape; anything else never reaches the renderer. */
export function readPageState(value: unknown): BrowserPageState | undefined {
  if (!isRecord(value)) return undefined;
  const text = ['sessionId', 'pageId', 'url', 'title'] as const;
  const flags = [
    'loading',
    'canGoBack',
    'canGoForward',
    'agentActive',
    'userInControl',
    'closed',
  ] as const;
  if (!text.every((key) => typeof value[key] === 'string')) return undefined;
  if (!flags.every((key) => typeof value[key] === 'boolean')) return undefined;
  const assist = value.assist;
  if (
    assist !== undefined &&
    !(
      isRecord(assist) &&
      typeof assist.reason === 'string' &&
      assist.reason.length <= 1_000 &&
      typeof assist.deadline === 'number' &&
      Number.isFinite(assist.deadline)
    )
  )
    return undefined;
  const notice = value.notice;
  if (
    notice !== undefined &&
    !(
      isRecord(notice) &&
      typeof notice.kind === 'string' &&
      NOTICES.has(notice.kind) &&
      typeof notice.text === 'string'
    )
  )
    return undefined;
  return {
    sessionId: value.sessionId as string,
    pageId: value.pageId as string,
    url: value.url as string,
    title: value.title as string,
    loading: value.loading as boolean,
    canGoBack: value.canGoBack as boolean,
    canGoForward: value.canGoForward as boolean,
    agentActive: value.agentActive as boolean,
    userInControl: value.userInControl as boolean,
    closed: value.closed as boolean,
    ...(isRecord(assist)
      ? { assist: { reason: assist.reason as string, deadline: assist.deadline as number } }
      : {}),
    notice:
      notice === undefined
        ? undefined
        : {
            kind: (notice as { kind: NonNullable<BrowserPageState['notice']>['kind'] }).kind,
            text: (notice as { text: string }).text,
          },
  };
}

function readCommandResult(value: unknown): BrowserCommandResult {
  if (isRecord(value) && value.ok === false && typeof value.message === 'string')
    return { ok: false, message: value.message };
  if (isRecord(value) && value.ok === true)
    return { ok: true, state: value.state === undefined ? undefined : readPageState(value.state) };
  return { ok: false, message: '浏览器没有回应' };
}

export function browserBridge(
  invoke: <C extends AgentMainCommand>(
    command: C,
    request: AgentMainParams[C],
  ) => Promise<ResultEnvelope<AgentMainResult[C]>>,
): BrowserApiV1 {
  return {
    command: async (command) => {
      try {
        return readCommandResult(await ipcRenderer.invoke(BROWSER_COMMAND_CHANNEL, command));
      } catch {
        return { ok: false, message: '浏览器没有回应' };
      }
    },
    pages: async () => {
      const value: unknown = await ipcRenderer.invoke(BROWSER_PAGES_CHANNEL).catch(() => []);
      return Array.isArray(value)
        ? value.flatMap((item) => {
            const state = readPageState(item);
            return state === undefined ? [] : [state];
          })
        : [];
    },
    onState: (listener) => {
      if (typeof listener !== 'function')
        throw new TypeError('Browser listener must be a function');
      const wrapped = (_event: IpcRendererEvent, value: unknown) => {
        const state = readPageState(value);
        if (state !== undefined) listener(state);
      };
      ipcRenderer.on(BROWSER_STATE_CHANNEL, wrapped);
      return () => ipcRenderer.removeListener(BROWSER_STATE_CHANNEL, wrapped);
    },
    getSettings: () => invoke('browser.settings.get', {}),
    updateSettings: (request) => invoke('browser.settings.update', request),
    readScreenshot: (request) => invoke('browser.screenshot.read', request),
  };
}
