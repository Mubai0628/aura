/** Releasing a native page saves its address and its own rules, never a user navigation. */
import type { BrowserPageState } from '../../shared/browser-api.js';
import { navigationVerdict, refusalText, type BrowserPolicy, type PageAccess } from './policy.js';

interface SavedPage {
  state: BrowserPageState;
  access: PageAccess;
}
const key = (session: string, page: string) => `${session}\u0000${page}`;
export interface PageNavigation {
  url: string;
  byUser: boolean;
}

export class ReleasedPages {
  readonly #pages = new Map<string, SavedPage>();

  remember(state: BrowserPageState, access: PageAccess): void {
    this.#pages.set(key(state.sessionId, state.pageId), {
      state: { ...state },
      access: { ...access, approvedHosts: [...access.approvedHosts] },
    });
  }

  state(session: string, page: string): BrowserPageState | undefined {
    const saved = this.#pages.get(key(session, page));
    return saved === undefined ? undefined : { ...saved.state };
  }

  restore(
    session: string,
    page: string,
    policy: BrowserPolicy,
    access?: PageAccess,
    navigation?: PageNavigation,
  ): SavedPage | undefined {
    const id = key(session, page);
    const saved = this.#pages.get(id);
    if (saved === undefined) return undefined;
    const ownAccess = access ?? saved.access;
    const url = navigation?.url ?? saved.state.url;
    const verdict = navigationVerdict(
      policy,
      navigation?.byUser ? { ...ownAccess, kind: 'user', readOnly: false } : ownAccess,
      url,
    );
    if (!verdict.allow) {
      saved.state = {
        ...saved.state,
        url,
        title: url === saved.state.url ? saved.state.title : '',
        closed: true,
        notice: { kind: 'blocked', text: refusalText(verdict) },
      };
      return { state: { ...saved.state }, access: ownAccess };
    }
    this.#pages.delete(id);
    return {
      access: ownAccess,
      state: {
        ...saved.state,
        url,
        title: url === saved.state.url ? saved.state.title : '',
        closed: false,
        loading: false,
        canGoBack: false,
        canGoForward: false,
        agentActive: false,
        userInControl: false,
        assist: undefined,
        notice: undefined,
      },
    };
  }

  forget(session: string, page: string): void {
    this.#pages.delete(key(session, page));
  }

  forgetSession(session: string): void {
    for (const [id, saved] of this.#pages)
      if (saved.state.sessionId === session) this.#pages.delete(id);
  }
}
