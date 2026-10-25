/**
 * The browser's site rules, as Core reads them (crates/aura-browser-domain/src/site.rs).
 *
 * A rule is a host name, `*.` and a host name (every subdomain, never the host itself), an IPv4
 * address or a bracketed IPv6 address, with an optional port; without a port it covers every
 * port and scheme. Both sides run fixtures/browser/site-rules.v1.json.
 */
export interface BrowserSite {
  scheme: string;
  host: string;
  port: number;
}

export interface SiteRule {
  subdomains: boolean;
  host: string;
  port?: number;
}

export type SiteRuleError = 'empty' | 'host' | 'port' | 'scheme';
export type SiteDecision = 'blocked' | 'allowed' | 'unlisted';

const DEFAULT_PORTS: Readonly<Record<string, number>> = {
  'http:': 80,
  'https:': 443,
  'ws:': 80,
  'wss:': 443,
};

/** The site of an http(s) or ws(s) address; other pages have none. */
export function siteOfUrl(address: string): BrowserSite | undefined {
  let url: URL;
  try {
    url = new URL(address);
  } catch {
    return undefined;
  }
  const defaultPort = DEFAULT_PORTS[url.protocol];
  if (defaultPort === undefined || url.hostname === '') return undefined;
  return {
    scheme: url.protocol.slice(0, -1),
    host: url.hostname.replace(/\.+$/u, '').toLowerCase(),
    port: url.port === '' ? defaultPort : Number(url.port),
  };
}

export function parseSiteRule(input: string): SiteRule | SiteRuleError {
  const text = input.trim();
  if (text === '') return 'empty';
  const schemeEnd = text.indexOf('://');
  if (schemeEnd >= 0) return fromAddress(text.slice(0, schemeEnd), text);
  const subdomains = text.startsWith('*.');
  const rest = subdomains ? text.slice(2) : text;
  if (/[@/\\?#]/u.test(rest)) return 'host';
  let host: string;
  let port: number | undefined;
  if (rest.startsWith('[')) {
    const close = rest.indexOf(']');
    if (close < 0) return 'host';
    host = rest.slice(0, close + 1);
    const after = rest.slice(close + 1);
    if (after !== '') {
      if (!after.startsWith(':')) return 'host';
      const parsed = parsePort(after.slice(1));
      if (typeof parsed === 'string') return parsed;
      port = parsed;
    }
  } else {
    const colon = rest.lastIndexOf(':');
    if (colon >= 0) {
      host = rest.slice(0, colon);
      if (host.includes(':')) return 'host';
      const parsed = parsePort(rest.slice(colon + 1));
      if (typeof parsed === 'string') return parsed;
      port = parsed;
    } else host = rest;
  }
  const normalized = normalizedHost(host, subdomains);
  if (normalized === undefined) return 'host';
  return port === undefined
    ? { subdomains, host: normalized }
    : { subdomains, host: normalized, port };
}

/** A pasted address: its host and a port written in it, nothing else. */
function fromAddress(scheme: string, text: string): SiteRule | SiteRuleError {
  if (!['http', 'https'].includes(scheme.toLowerCase())) return 'scheme';
  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return 'host';
  }
  if (url.username !== '' || url.password !== '') return 'host';
  const host = normalizedHost(url.hostname, false);
  if (host === undefined) return 'host';
  return url.port === ''
    ? { subdomains: false, host }
    : { subdomains: false, host, port: Number(url.port) };
}

function parsePort(written: string): number | SiteRuleError {
  if (!/^\d+$/u.test(written)) return 'port';
  const port = Number(written);
  return port >= 1 && port <= 65_535 ? port : 'port';
}

const IPV4 = /^\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}$/u;
const LABEL = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/u;

/** The host as a browser normalizes it, then held to letters, digits and hyphens. */
function normalizedHost(host: string, subdomains: boolean): string | undefined {
  let hostname: string;
  try {
    hostname = new URL(`http://${host}/`).hostname;
  } catch {
    return undefined;
  }
  if (hostname.startsWith('[')) return subdomains ? undefined : hostname;
  if (IPV4.test(hostname)) return subdomains ? undefined : hostname;
  const domain = hostname.replace(/\.+$/u, '').toLowerCase();
  if (domain === '' || domain.length > 253) return undefined;
  return domain.split('.').every((label) => LABEL.test(label)) ? domain : undefined;
}

export function ruleText(rule: SiteRule): string {
  return `${rule.subdomains ? '*.' : ''}${rule.host}${rule.port === undefined ? '' : `:${rule.port}`}`;
}

export function ruleMatches(rule: SiteRule, site: BrowserSite): boolean {
  if (rule.port !== undefined && rule.port !== site.port) return false;
  return rule.subdomains ? site.host.endsWith(`.${rule.host}`) : site.host === rule.host;
}

/** What the user's rules say about one site; a blocked site stays blocked. */
export function decideSite(
  site: BrowserSite,
  allowed: readonly SiteRule[],
  blocked: readonly SiteRule[],
): SiteDecision {
  if (blocked.some((rule) => ruleMatches(rule, site))) return 'blocked';
  return allowed.some((rule) => ruleMatches(rule, site)) ? 'allowed' : 'unlisted';
}

/** Rules already checked by Core; one that does not parse here is dropped. */
export function parsedRules(texts: readonly string[]): SiteRule[] {
  return texts.flatMap((text) => {
    const rule = parseSiteRule(text);
    return typeof rule === 'string' ? [] : [rule];
  });
}
