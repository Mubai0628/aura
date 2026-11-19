/**
 * What the built-in browser may load (capability decisions §2.3).
 *
 * The block list holds for every page, every mode, every redirect and every request a page makes.
 * A ReadOnly session's model page navigates to allowed sites and to hosts the user approved in
 * that session; other pages navigate anywhere not blocked. A public page cannot reach this
 * computer or the local network.
 */
import { hostClass } from './address.js';
import { decideSite, siteOfUrl, type SiteRule } from './site-rules.js';

/** Why the browser is closed: turned off, its settings need recovery, or not read from Core yet. */
export type ClosedBecause = 'off' | 'issue' | 'unread';

export interface BrowserPolicy {
  enabled: boolean;
  /** Why it is closed, when it is; turned off when unsaid. */
  closedBecause?: ClosedBecause | undefined;
  allowed: readonly SiteRule[];
  blocked: readonly SiteRule[];
  /** The user allowed models to debug pages (a script in the page, its network log). */
  debugging?: boolean;
  debuggingBlocked?: boolean;
  revision?: number;
}

/** Who a page belongs to and, for a model page, what its session allows. */
export interface PageAccess {
  kind: 'model' | 'user';
  readOnly: boolean;
  /** Hosts the user approved for this ReadOnly session. */
  approvedHosts: readonly string[];
  /**
   * A workspace folder, as a `file:` address ending in `/`: files under it open. A model page's
   * comes from Core; a user's page has one once the user opened a workspace page in it,
   * after Main found where that file really is.
   */
  fileRoot?: string | undefined;
}

/** Whether `url` is a file inside the workspace folder (compared as Windows compares paths). */
export function insideFileRoot(url: string, fileRoot: string | undefined): boolean {
  if (fileRoot === undefined || !url.startsWith('file:')) return false;
  let path: string;
  let root: string;
  try {
    path = decodeURIComponent(new URL(url).pathname).toLowerCase();
    root = decodeURIComponent(new URL(fileRoot).pathname).toLowerCase();
  } catch {
    return false;
  }
  return root.endsWith('/') && path.startsWith(root);
}

export type Refusal = 'disabled' | 'blocked' | 'read-only' | 'private-from-public' | 'scheme';
export type Verdict =
  | { allow: true }
  | { allow: false; reason: Refusal; site?: string; because?: ClosedBecause | undefined };

const ALLOW: Verdict = { allow: true };

/** A top-level navigation: typed, followed, scripted or redirected. */
export function navigationVerdict(policy: BrowserPolicy, page: PageAccess, url: string): Verdict {
  if (!policy.enabled) return { allow: false, reason: 'disabled', because: policy.closedBecause };
  if (url === 'about:blank') return ALLOW;
  if (url.startsWith('file:'))
    return insideFileRoot(url, page.fileRoot) ? ALLOW : { allow: false, reason: 'scheme' };
  const site = siteOfUrl(url);
  if (site === undefined || site.scheme.startsWith('ws')) return { allow: false, reason: 'scheme' };
  const decision = decideSite(site, policy.allowed, policy.blocked);
  if (decision === 'blocked') return { allow: false, reason: 'blocked', site: site.host };
  if (
    page.kind === 'model' &&
    page.readOnly &&
    decision !== 'allowed' &&
    !page.approvedHosts.includes(site.host)
  )
    return { allow: false, reason: 'read-only', site: site.host };
  return ALLOW;
}

/** A request a page makes: a script, an image, a fetch, a socket, a frame. */
export function subrequestVerdict(
  policy: BrowserPolicy,
  topUrl: string,
  url: string,
  fileRoot?: string,
): Verdict {
  if (!policy.enabled) return { allow: false, reason: 'disabled', because: policy.closedBecause };
  // A workspace page loads its own files; no page reaches any other file.
  if (url.startsWith('file:'))
    return insideFileRoot(url, fileRoot) && insideFileRoot(topUrl, fileRoot)
      ? ALLOW
      : { allow: false, reason: 'scheme' };
  const site = siteOfUrl(url);
  if (site === undefined) return ALLOW;
  if (decideSite(site, policy.allowed, policy.blocked) === 'blocked')
    return { allow: false, reason: 'blocked', site: site.host };
  const top = siteOfUrl(topUrl);
  if (top !== undefined && hostClass(top.host) === 'public' && hostClass(site.host) !== 'public')
    return { allow: false, reason: 'private-from-public', site: site.host };
  return ALLOW;
}

const CLOSED: Readonly<Record<ClosedBecause, string>> = {
  off: '内置浏览器已在设置中关闭',
  issue: '浏览器设置需要恢复，内置浏览器暂不能用',
  unread: '浏览器设置还没从 Aura Core 读到',
};

/** Why the browser did not act, `what` it did not do, and what to do next. */
export function closedText(policy: BrowserPolicy, what: string): string {
  switch (policy.closedBecause ?? 'off') {
    case 'unread':
      return `浏览器设置还没从 Aura Core 读到（Core 可能正在启动），${what}；稍后再试。`;
    case 'issue':
      return `浏览器设置需要恢复，内置浏览器暂不能用，${what}；请到 设置 → 网络与浏览器 查看。`;
    case 'off':
      return `内置浏览器已在设置中关闭，${what}。到 设置 → 网络与浏览器 打开“启用内置浏览器”后再试。`;
  }
}

/** What the page shows when a load is refused. */
export function refusalText(verdict: Exclude<Verdict, { allow: true }>): string {
  const site = verdict.site ?? '';
  switch (verdict.reason) {
    case 'disabled':
      return CLOSED[verdict.because ?? 'off'];
    case 'blocked':
      return `${site} 已在浏览器设置中被阻止`;
    case 'read-only':
      return `只读会话打开 ${site} 需要批准`;
    case 'private-from-public':
      return `公网页面不能访问本机或局域网地址 ${site}`;
    case 'scheme':
      return '只能打开网页地址';
  }
}
