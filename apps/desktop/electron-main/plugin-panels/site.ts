/**
 * Where a panel page lives and what it may reach: its own origin `aura-plugin://<plugin>`, served
 * from the plugin's `panel/` folder under a strict policy, and only the network origins the plugin
 * declared in `[panel] network`. Nothing else leaves the page.
 */
import { join } from 'node:path';
import { panelFile } from './files.js';

export const PANEL_SCHEME = 'aura-plugin';

/** For `protocol.registerSchemesAsPrivileged`: a standard, secure origin per plugin. */
export const PANEL_SCHEME_PRIVILEGES = {
  scheme: PANEL_SCHEME,
  privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true },
} as const;

/** What Core said the panel is when it was opened. */
export interface PanelSite {
  plugin: string;
  title: string;
  /** The plugin's folder. */
  folder: string;
  /** Relative to the folder, inside `panel/`. */
  entry: string;
  /** `https://host[:port]` or `wss://host[:port]`. */
  network: readonly string[];
  capabilities: readonly string[];
}

const origin = (address: string) => address.replace(/\/$/u, '').toLowerCase();

export function panelOrigin(plugin: string): string {
  return `${PANEL_SCHEME}://${plugin}`;
}

/** The page address of the panel's entry (`panel/index.html` → `aura-plugin://name/index.html`). */
export function entryUrl(site: PanelSite): string {
  const inside = site.entry.replace(/^panel\//u, '');
  return `${panelOrigin(site.plugin)}/${inside.split('/').map(encodeURIComponent).join('/')}`;
}

export function panelCsp(site: Pick<PanelSite, 'network'>): string {
  const network = site.network.map(origin).join(' ');
  return [
    "default-src 'none'",
    "script-src 'self'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: blob:",
    "font-src 'self' data:",
    "media-src 'self' blob:",
    `connect-src 'self'${network === '' ? '' : ` ${network}`}`,
    "worker-src 'self'",
    "frame-src 'none'",
    "child-src 'none'",
    "object-src 'none'",
    "manifest-src 'none'",
    "base-uri 'none'",
    "form-action 'none'",
    "frame-ancestors 'none'",
  ].join('; ');
}

/** Whether the page may make this request: its own files, or a declared origin. */
export function requestAllowed(url: string, site: Pick<PanelSite, 'plugin' | 'network'>): boolean {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return false;
  }
  if (parsed.protocol === `${PANEL_SCHEME}:`) return parsed.host === site.plugin;
  if (parsed.protocol !== 'https:' && parsed.protocol !== 'wss:') return false;
  return site.network.some((declared) => origin(declared) === parsed.origin.toLowerCase());
}

/** The answer to one request of the panel's own scheme. */
export async function panelResponse(
  request: { method: string; url: string },
  site: PanelSite | undefined,
): Promise<Response> {
  const refused = (status: number, text: string) =>
    new Response(text, {
      status,
      headers: { 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'no-store' },
    });
  if (site === undefined) return refused(403, '这个面板已关闭。');
  let url: URL;
  try {
    url = new URL(request.url);
  } catch {
    return refused(400, '地址无效。');
  }
  if (url.protocol !== `${PANEL_SCHEME}:` || url.host !== site.plugin)
    return refused(403, '只能读这个插件自己的面板文件。');
  if (request.method !== 'GET') return refused(405, '面板文件只能读。');
  const file = await panelFile(join(site.folder, 'panel'), url.pathname);
  if (file === undefined) return refused(404, '面板里没有这个文件，或它不能被读。');
  return new Response(new Uint8Array(file.body), {
    headers: {
      'Content-Type': file.type,
      'Content-Security-Policy': panelCsp(site),
      'Cache-Control': 'no-store',
      'X-Content-Type-Options': 'nosniff',
      'Cross-Origin-Resource-Policy': 'same-origin',
      'Referrer-Policy': 'no-referrer',
    },
  });
}
