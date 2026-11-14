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
  PAGE_TEXT_CHARS,
  browserNotDone,
  type BrowserApiV1,
  type BrowserCommandResult,
  type BrowserPageState,
  type PageText,
} from '../shared/browser-api.js';
import type { ResultEnvelope } from '../shared/aura-api.js';

const NOTICES = new Set(['blocked', 'download', 'popup', 'failed', 'crashed', 'closed']);
/** A page's picture is only ever a JPEG Main encoded. */
const PICTURE_PREFIX = 'data:image/jpeg;base64,';
/** The id a page the user handed over has in its conversation, as Core takes it. */
const HANDED_ID = /^p[1-9][0-9]{0,3}$/u;
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
  const handedAs = value.handedAs;
  if (handedAs !== undefined && !(typeof handedAs === 'string' && HANDED_ID.test(handedAs)))
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
    ...(typeof handedAs === 'string' ? { handedAs } : {}),
    notice:
      notice === undefined
        ? undefined
        : {
            kind: (notice as { kind: NonNullable<BrowserPageState['notice']>['kind'] }).kind,
            text: (notice as { text: string }).text,
          },
  };
}

/** A page's text as Main read it for "@", in exactly its shape. */
function readPageText(value: unknown): PageText | undefined {
  return isRecord(value) &&
    typeof value.title === 'string' &&
    typeof value.url === 'string' &&
    typeof value.text === 'string' &&
    value.text.length <= PAGE_TEXT_CHARS &&
    typeof value.truncated === 'boolean'
    ? { title: value.title, url: value.url, text: value.text, truncated: value.truncated }
    : undefined;
}

function readCommandResult(value: unknown): BrowserCommandResult {
  if (isRecord(value) && value.ok === false && typeof value.message === 'string')
    return { ok: false, message: value.message };
  if (isRecord(value) && value.ok === true)
    return {
      ok: true,
      state: value.state === undefined ? undefined : readPageState(value.state),
      picture:
        typeof value.picture === 'string' && value.picture.startsWith(PICTURE_PREFIX)
          ? value.picture
          : undefined,
      text: value.text === undefined ? undefined : readPageText(value.text),
    };
  return {
    ok: false,
    message:
      '这次浏览器操作的结果读不懂（Aura 内部出错），操作可能没有完成；请重试，仍不行请重启 Aura。',
  };
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
        return {
          ok: false,
          message: `浏览器没有回应，${browserNotDone(command.kind)}；请稍后重试，仍不行时重启 Aura。`,
        };
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
