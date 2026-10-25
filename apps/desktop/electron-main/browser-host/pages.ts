/**
 * The pages of Aura's built-in browser. Each page is a WebContentsView kept in the Aura
 * window (shown in the right sidebar, otherwise hidden but still running); a session's model
 * page and the tabs the user opened are kept apart. Every load passes the site policy.
 */
import {
  MODEL_PAGE_ID,
  type BrowserAssistState,
  type BrowserBounds,
  type BrowserNoticeKind,
  type BrowserPageState,
} from '../../shared/browser-api.js';
import { addressFromInput } from './address.js';
import { wirePage } from './page-events.js';
import { ReleasedPages, type PageNavigation } from './released-pages.js';
import {
  navigationVerdict,
  refusalText,
  subrequestVerdict,
  type BrowserPolicy,
  type PageAccess,
} from './policy.js';

type Listener = (...args: never[]) => void;

/** The parts of Electron's WebContents a page uses; tests pass a fake. */
export interface PageContents {
  readonly id: number;
  loadURL(url: string): Promise<void>;
  getURL(): string;
  getTitle(): string;
  isLoading(): boolean;
  isDestroyed(): boolean;
  stop(): void;
  reload(): void;
  close(): void;
  readonly navigationHistory: {
    canGoBack(): boolean;
    canGoForward(): boolean;
    goBack(): void;
    goForward(): void;
  };
  setWindowOpenHandler(handler: (details: { url: string }) => { action: 'deny' }): void;
  on(event: string, listener: Listener): unknown;
  /** The page's DevTools, detached, for the user. */
  openDevTools(options: { mode: 'detach'; activate: boolean; title?: string }): void;
}
export interface PageView {
  readonly webContents: PageContents;
  setBounds(bounds: BrowserBounds): void;
  setVisible(visible: boolean): void;
}
export interface PageViews {
  create(): PageView;
  attach(view: PageView): void;
  detach(view: PageView): void;
}

export interface BrowserRequest {
  webContentsId?: number | undefined;
  resourceType: string;
  url: string;
}

interface Page {
  readonly key: string;
  readonly view: PageView;
  access: PageAccess;
  state: BrowserPageState;
  lastUsed: number;
  shown: boolean;
  /** Where the sidebar last showed it (window coordinates). */
  bounds?: BrowserBounds;
  /** Set while a navigation the user typed is loading: it is judged as the user's. */
  userNavigation: boolean;
  /** Why a recent load was refused, by address, for the failure that follows. */
  readonly refusals: Map<string, string>;
  /** What the user did to the model's page since the model last looked at it. */
  userHad?: UserTouch | undefined;
}

/** How the user changed the model's page: took it over, or navigated it from the toolbar. */
export type UserTouch = 'took_over' | 'navigated';

const USER: PageAccess = { kind: 'user', readOnly: false, approvedHosts: [] };
const pageKey = (sessionId: string, pageId: string) => `${sessionId}\u0000${pageId}`;

export class BrowserPages {
  readonly #views: PageViews;
  readonly #limit: number;
  readonly #now: () => number;
  readonly #pages = new Map<string, Page>();
  readonly #released = new ReleasedPages();
  readonly #listeners = new Set<(state: BrowserPageState) => void>();
  readonly #shown = new Set<(sessionId: string, pageId: string, bounds: BrowserBounds) => void>();
  // Closed until the settings are read: a browser must not run on rules it has not seen.
  #policy: BrowserPolicy = { enabled: false, allowed: [], blocked: [] };

  constructor(views: PageViews, options: { limit?: number; now?: () => number } = {}) {
    this.#views = views;
    this.#limit = options.limit ?? 8;
    this.#now = options.now ?? Date.now;
  }

  get policy(): BrowserPolicy {
    return this.#policy;
  }

  setPolicy(policy: BrowserPolicy): void {
    this.#policy = policy;
    if (!policy.enabled)
      for (const page of [...this.#pages.values()])
        this.#close(page, refusalText({ allow: false, reason: 'disabled' }));
  }

  onState(listener: (state: BrowserPageState) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  states(): BrowserPageState[] {
    return [...this.#pages.values()].map((page) => ({ ...page.state }));
  }

  state(sessionId: string, pageId: string): BrowserPageState | undefined {
    const page = this.#pages.get(pageKey(sessionId, pageId));
    return page === undefined ? this.#released.state(sessionId, pageId) : { ...page.state };
  }

  /** The page, created on first use. */
  open(sessionId: string, pageId: string, access?: PageAccess, url?: string): BrowserPageState {
    const page = this.#open(
      sessionId,
      pageId,
      access,
      url === undefined ? undefined : { url, byUser: false },
    );
    return { ...(page?.state ?? this.#released.state(sessionId, pageId)!) };
  }

  show(sessionId: string, pageId: string, bounds: BrowserBounds): BrowserPageState {
    const page = this.#open(sessionId, pageId);
    if (page === undefined) return this.#released.state(sessionId, pageId)!;
    for (const other of this.#pages.values())
      if (other !== page && other.shown) {
        other.shown = false;
        other.view.setVisible(false);
      }
    page.view.setBounds(bounds);
    page.view.setVisible(true);
    page.shown = true;
    page.bounds = bounds;
    for (const listener of this.#shown) listener(sessionId, pageId, bounds);
    return { ...page.state };
  }

  /** Told each time a page is shown, with where. */
  onShown(
    listener: (sessionId: string, pageId: string, bounds: BrowserBounds) => void,
  ): () => void {
    this.#shown.add(listener);
    return () => this.#shown.delete(listener);
  }

  /** The page's web contents, if it is open. */
  contents(sessionId: string, pageId: string): PageContents | undefined {
    return this.#pages.get(pageKey(sessionId, pageId))?.view.webContents;
  }

  /** Where the sidebar last showed the page. */
  bounds(sessionId: string, pageId: string): BrowserBounds | undefined {
    return this.#pages.get(pageKey(sessionId, pageId))?.bounds;
  }

  /**
   * A model's request for the user's help starts or ends. Meanwhile the page is the user's: its
   * navigations are judged as theirs, and their touch does not take it over from the model.
   */
  setAssist(sessionId: string, pageId: string, assist: BrowserAssistState | undefined): void {
    const page = this.#pages.get(pageKey(sessionId, pageId));
    if (page === undefined || pageId !== MODEL_PAGE_ID) return;
    this.#update(page, assist === undefined ? { assist, userInControl: false } : { assist });
  }

  /** A model action is running on the page now, or has ended. */
  setAgentActive(sessionId: string, pageId: string, active: boolean): void {
    const page = this.#pages.get(pageKey(sessionId, pageId));
    if (page !== undefined && page.state.agentActive !== active)
      this.#update(page, { agentActive: active });
  }

  /** The session and page a web contents belongs to. */
  owner(webContentsId: number): { sessionId: string; pageId: string } | undefined {
    const page = this.#byContents(webContentsId);
    return page === undefined
      ? undefined
      : { sessionId: page.state.sessionId, pageId: page.state.pageId };
  }

  hide(sessionId: string, pageId: string): void {
    const page = this.#pages.get(pageKey(sessionId, pageId));
    if (page === undefined || !page.shown) return;
    page.shown = false;
    page.view.setVisible(false);
  }

  /** What the user typed in the address bar. */
  navigate(sessionId: string, pageId: string, input: string): string | undefined {
    const url = addressFromInput(input);
    if (url === undefined) return '请输入网址，例如 example.com 或 localhost:5173';
    const page = this.#open(sessionId, pageId, undefined, { url, byUser: true });
    if (page === undefined) return this.#released.state(sessionId, pageId)?.notice?.text;
    const verdict = navigationVerdict(this.#policy, USER, url);
    if (!verdict.allow) {
      this.#update(page, { notice: { kind: 'blocked', text: refusalText(verdict) } });
      return refusalText(verdict);
    }
    page.userNavigation = true;
    if (pageId === MODEL_PAGE_ID) page.userHad ??= 'navigated';
    this.#update(page, { notice: undefined });
    void page.view.webContents.loadURL(url).catch(() => undefined);
    return undefined;
  }

  /**
   * A workspace web page the user opened from Aura, in a page of their own. Main found the
   * file inside the workspace first; the page may then open that workspace's files, no others.
   */
  openFile(sessionId: string, pageId: string, url: string, fileRoot: string): string | undefined {
    if (pageId === MODEL_PAGE_ID) return '只能在你自己的页面中打开工作区文件';
    const page = this.#open(sessionId, pageId, { ...USER, fileRoot }, { url, byUser: true });
    if (page === undefined) return this.#released.state(sessionId, pageId)?.notice?.text;
    page.access = { ...USER, fileRoot };
    const verdict = navigationVerdict(this.#policy, page.access, url);
    if (!verdict.allow) {
      this.#update(page, { notice: { kind: 'blocked', text: refusalText(verdict) } });
      return refusalText(verdict);
    }
    page.userNavigation = true;
    this.#update(page, { notice: undefined });
    void page.view.webContents.loadURL(url).catch(() => undefined);
    return undefined;
  }

  /** A model's navigation: judged by the page's model access, never as the user's own typing. */
  loadForModel(sessionId: string, pageId: string, url: string): Promise<void> | undefined {
    const page = this.#pages.get(pageKey(sessionId, pageId));
    if (page === undefined) return undefined;
    page.userNavigation = false;
    this.#update(page, { notice: undefined });
    return page.view.webContents.loadURL(url);
  }

  /** Why the last load of this address was refused, if it was. */
  refusal(sessionId: string, pageId: string, url: string): string | undefined {
    return this.#pages.get(pageKey(sessionId, pageId))?.refusals.get(url);
  }

  /** Back, forward, reload or stop: the user's from the toolbar, or a model's (`byModel`). */
  history(
    sessionId: string,
    pageId: string,
    kind: 'back' | 'forward' | 'reload' | 'stop',
    byModel = false,
  ): void {
    const page = this.#pages.get(pageKey(sessionId, pageId));
    if (page === undefined) return;
    const contents = page.view.webContents;
    page.userNavigation = !byModel && kind !== 'stop';
    if (page.userNavigation && pageId === MODEL_PAGE_ID) page.userHad ??= 'navigated';
    if (kind === 'back' && contents.navigationHistory.canGoBack())
      contents.navigationHistory.goBack();
    else if (kind === 'forward' && contents.navigationHistory.canGoForward())
      contents.navigationHistory.goForward();
    else if (kind === 'reload') contents.reload();
    else if (kind === 'stop') contents.stop();
  }

  close(sessionId: string, pageId: string): void {
    this.#released.forget(sessionId, pageId);
    const page = this.#pages.get(pageKey(sessionId, pageId));
    if (page !== undefined) this.#close(page, undefined);
  }

  /** A deleted conversation takes its pages with it. */
  closeSession(sessionId: string): void {
    this.#released.forgetSession(sessionId);
    for (const page of [...this.#pages.values()])
      if (page.state.sessionId === sessionId) this.#close(page, undefined);
  }

  setUserControl(sessionId: string, pageId: string, user: boolean): void {
    const page = this.#pages.get(pageKey(sessionId, pageId));
    if (page === undefined || pageId !== MODEL_PAGE_ID) return;
    if (user) page.userHad = 'took_over';
    this.#update(page, { userInControl: user });
  }

  /**
   * What the user did to the model's page since
   * the model last looked at it, and so may have changed there.
   */
  userHad(sessionId: string): UserTouch | undefined {
    return this.#pages.get(pageKey(sessionId, MODEL_PAGE_ID))?.userHad;
  }

  /** The model has seen its page as it is now, the user no longer holding it. */
  modelLooked(sessionId: string): void {
    const page = this.#pages.get(pageKey(sessionId, MODEL_PAGE_ID));
    if (page !== undefined && !page.state.userInControl) page.userHad = undefined;
  }

  dismissNotice(sessionId: string, pageId: string): void {
    const page = this.#pages.get(pageKey(sessionId, pageId));
    if (page !== undefined) this.#update(page, { notice: undefined });
  }

  /** A model page's access, as Core states it with each command. */
  setAccess(sessionId: string, pageId: string, access: PageAccess): void {
    const page = this.#pages.get(pageKey(sessionId, pageId));
    if (page !== undefined) page.access = access;
  }

  notice(webContentsId: number, kind: BrowserNoticeKind, text: string): void {
    const page = this.#byContents(webContentsId);
    if (page !== undefined) this.#update(page, { notice: { kind, text } });
  }

  /** The session's one choke point: every request of every page passes here. */
  allowRequest(request: BrowserRequest): boolean {
    const page =
      request.webContentsId === undefined ? undefined : this.#byContents(request.webContentsId);
    if (request.resourceType === 'mainFrame') {
      if (page === undefined) return false;
      const verdict = navigationVerdict(this.#policy, this.#access(page), request.url);
      if (verdict.allow) return true;
      page.refusals.set(request.url, refusalText(verdict));
      return false;
    }
    const top = page?.view.webContents.getURL() ?? '';
    return subrequestVerdict(this.#policy, top, request.url, page?.access.fileRoot).allow;
  }

  /** Whose rules a navigation of the page follows now: a user's page always follows its own. */
  #access(page: Page): PageAccess {
    if (page.access.kind === 'user') return page.access;
    return page.userNavigation || page.state.assist !== undefined ? USER : page.access;
  }

  #byContents(id: number): Page | undefined {
    for (const page of this.#pages.values()) if (page.view.webContents.id === id) return page;
    return undefined;
  }

  #open(
    sessionId: string,
    pageId: string,
    access?: PageAccess,
    navigation?: PageNavigation,
  ): Page | undefined {
    const key = pageKey(sessionId, pageId);
    const existing = this.#pages.get(key);
    if (existing !== undefined) {
      existing.lastUsed = this.#now();
      return existing;
    }
    const restored = this.#released.restore(sessionId, pageId, this.#policy, access, navigation);
    if (restored?.state.closed) {
      for (const listener of this.#listeners) listener({ ...restored.state });
      return undefined;
    }
    this.#evict();
    const view = this.#views.create();
    const page: Page = {
      key,
      view,
      access:
        restored?.access ??
        access ??
        (pageId === MODEL_PAGE_ID ? { kind: 'model', readOnly: true, approvedHosts: [] } : USER),
      state: restored?.state ?? {
        sessionId,
        pageId,
        url: 'about:blank',
        title: '',
        loading: false,
        canGoBack: false,
        canGoForward: false,
        agentActive: false,
        userInControl: false,
        closed: false,
      },
      lastUsed: this.#now(),
      shown: false,
      userNavigation: navigation?.byUser ?? false,
      refusals: new Map(),
    };
    this.#pages.set(key, page);
    wirePage(page, {
      policy: () => this.#policy,
      access: () => this.#access(page),
      update: (change) => this.#update(page, change),
    });
    this.#views.attach(view);
    view.setVisible(false);
    // New pages start blank for DevTools; a released page reloads its own checked address.
    if (!navigation?.byUser)
      void view.webContents
        .loadURL(navigation === undefined ? page.state.url : 'about:blank')
        .catch(() => undefined);
    this.#emit(page);
    return page;
  }

  /** Over the limit, the page used longest ago that is not on screen is released. */
  #evict(): void {
    while (this.#pages.size >= this.#limit) {
      const candidates = [...this.#pages.values()].filter(
        (page) => !page.shown && !page.state.agentActive && page.state.assist === undefined,
      );
      const oldest = candidates.sort((a, b) => a.lastUsed - b.lastUsed)[0];
      if (oldest === undefined) return;
      this.#close(oldest, '为节省内存已关闭这个页面，再次打开即可');
      this.#released.remember(oldest.state, oldest.access);
    }
  }

  #close(page: Page, why: string | undefined): void {
    if (this.#pages.get(page.key) !== page) return;
    this.#pages.delete(page.key);
    this.#views.detach(page.view);
    if (!page.view.webContents.isDestroyed()) page.view.webContents.close();
    page.state = {
      ...page.state,
      closed: true,
      loading: false,
      agentActive: false,
      notice: why === undefined ? undefined : { kind: 'closed', text: why },
    };
    this.#emit(page);
  }

  #update(page: Page, change: Partial<BrowserPageState>): void {
    if (this.#pages.get(page.key) !== page) return;
    page.state = { ...page.state, ...change };
    if (!page.view.webContents.isDestroyed()) page.state.title ||= page.view.webContents.getTitle();
    this.#emit(page);
  }

  #emit(page: Page): void {
    for (const listener of this.#listeners) listener({ ...page.state });
  }
}
