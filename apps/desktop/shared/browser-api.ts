import type { AgentMainParams, AgentMainResult } from './generated/routes.js';
import type { ResultEnvelope } from './aura-api.js';

/**
 * Aura's built-in browser. Pages live in Main; the renderer shows one at a time in the right
 * sidebar and sends the user's own commands. A model's commands come from Core, never from here.
 */
export const BROWSER_COMMAND_CHANNEL = 'aura:v1:browser-command';
export const BROWSER_STATE_CHANNEL = 'aura:v1:browser-state';
export const BROWSER_PAGES_CHANNEL = 'aura:v1:browser-pages';

/** The page a session's model drives; every other page id is a tab the user opened. */
export const MODEL_PAGE_ID = 'model';

export interface BrowserBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type BrowserNoticeKind = 'blocked' | 'download' | 'popup' | 'failed' | 'crashed' | 'closed';

/** A model asked the user to act on its page (browser.request_user) and waits for the answer. */
export interface BrowserAssistState {
  /** The model's sentence: what the user should do. */
  reason: string;
  /** When the model stops waiting, in milliseconds since the epoch (this computer's clock). */
  deadline: number;
}

export interface BrowserPageState {
  sessionId: string;
  pageId: string;
  url: string;
  title: string;
  loading: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  /** A model action is running on this page now. */
  agentActive: boolean;
  /** The user took the page over: the model's input is refused until they hand it back. */
  userInControl: boolean;
  /** The model waits for the user to act on the page; the page is the user's meanwhile. */
  assist?: BrowserAssistState | undefined;
  notice?: { kind: BrowserNoticeKind; text: string } | undefined;
  /** The page no longer exists (closed, or released to save memory). */
  closed: boolean;
}

export type BrowserUserCommand =
  | { kind: 'open'; sessionId: string; pageId: string }
  | { kind: 'navigate'; sessionId: string; pageId: string; input: string }
  | {
      kind:
        | 'back'
        | 'forward'
        | 'reload'
        | 'stop'
        | 'hide'
        | 'close'
        | 'take-control'
        | 'release-control'
        /** The user's answer to a model's request for help: done, or declined. */
        | 'assist-done'
        | 'assist-decline'
        /** The page's own Chromium DevTools, in a window of their own, for the user. */
        | 'devtools'
        /** A picture of the page as it is now, shown or hidden, to draw where it would be. */
        | 'picture';
      sessionId: string;
      pageId: string;
    }
  | { kind: 'show'; sessionId: string; pageId: string; bounds: BrowserBounds }
  /** A workspace web page, named by its workspace path; Main finds where it really is. */
  | { kind: 'open-file'; sessionId: string; pageId: string; workspaceId: string; path: string }
  | { kind: 'dismiss-notice'; sessionId: string; pageId: string }
  /** Signs out of every site and empties the cache of the browser's own profile. */
  | { kind: 'clear-data' };

export type BrowserCommandResult =
  | {
      ok: true;
      state?: BrowserPageState | undefined;
      /** For 'picture': a JPEG data URL; absent when the page could not be pictured. */
      picture?: string | undefined;
    }
  | { ok: false; message: string };

type Request<M extends keyof AgentMainParams> = (
  request: AgentMainParams[M],
) => Promise<ResultEnvelope<AgentMainResult[M]>>;

export interface BrowserApiV1 {
  command(command: BrowserUserCommand): Promise<BrowserCommandResult>;
  /** Every page Main holds now. */
  pages(): Promise<BrowserPageState[]>;
  onState(listener: (state: BrowserPageState) => void): () => void;
  getSettings: () => ReturnType<Request<'browser.settings.get'>>;
  updateSettings: Request<'browser.settings.update'>;
  /** A screenshot a browser call of the conversation took. */
  readScreenshot: Request<'browser.screenshot.read'>;
}
