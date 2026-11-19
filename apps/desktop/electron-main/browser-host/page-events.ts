/** Native page events and popup policy; the collection owns the resulting state updates. */
import type { BrowserPageState } from '../../shared/browser-api.js';
import type { PageView } from './pages.js';
import { navigationVerdict, refusalText, type BrowserPolicy, type PageAccess } from './policy.js';
import { loadFailureText } from './load-failure.js';

type Listener = (...args: never[]) => void;
interface EventPage {
  view: PageView;
  userNavigation: boolean;
  refusals: Map<string, string>;
}
interface PageEvents {
  policy(): BrowserPolicy;
  access(): PageAccess;
  documentChanged(): void;
  update(change: Partial<BrowserPageState>): void;
}

export function wirePage(
  page: EventPage,
  { policy, access, documentChanged, update }: PageEvents,
): void {
  const contents = page.view.webContents;
  const history = () => ({
    canGoBack: contents.navigationHistory.canGoBack(),
    canGoForward: contents.navigationHistory.canGoForward(),
  });
  contents.setWindowOpenHandler(({ url }) => {
    const verdict = navigationVerdict(policy(), access(), url);
    if (verdict.allow) void contents.loadURL(url).catch(() => undefined);
    else update({ notice: { kind: 'popup', text: refusalText(verdict) } });
    return { action: 'deny' };
  });
  contents.on('did-start-loading', () => update({ loading: true }));
  contents.on('did-stop-loading', () => {
    page.userNavigation = false;
    update({ loading: false, ...history() });
  });
  contents.on('did-navigate', ((_event: unknown, url: string) => {
    documentChanged();
    page.refusals.clear();
    update({ url, notice: undefined, ...history() });
  }) as Listener);
  contents.on('did-navigate-in-page', ((_event: unknown, url: string, isMainFrame: boolean) => {
    if (!isMainFrame) return;
    documentChanged();
    update({ url, ...history() });
  }) as Listener);
  contents.on('page-title-updated', ((_event: unknown, title: string) =>
    update({ title })) as Listener);
  contents.on('did-fail-load', ((
    _event: unknown,
    code: number,
    description: string,
    url: string,
    isMainFrame: boolean,
  ) => {
    if (!isMainFrame || code === -3) return;
    const refused = page.refusals.get(url);
    update({
      loading: false,
      notice: refused
        ? { kind: 'blocked', text: refused }
        : { kind: 'failed', text: loadFailureText(code, description) },
    });
  }) as Listener);
  contents.on('render-process-gone', () => {
    documentChanged();
    update({ loading: false, notice: { kind: 'crashed', text: '页面已崩溃，刷新可重新加载' } });
  });
}
