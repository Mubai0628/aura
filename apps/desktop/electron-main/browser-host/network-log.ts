/**
 * The requests a page made, as DevTools reports them, for `browser.network`. Cookie,
 * authorization and token values never leave here: header values and address parts that carry
 * them are hidden before anything is kept.
 */
import type { BrowserNetworkEntryV1 } from '@aura/contracts/generated';

type Params = Record<string, unknown>;

const KEEP = 300;
const HEADERS = 40;
const LONGEST = 300;
export const HIDDEN = '（已隐藏）';

/** Header names whose values are credentials or carry them. */
const SECRET_HEADER =
  /^(cookie|set-cookie|authorization|proxy-authorization)$|token|secret|api[-_]?key|session|csrf|xsrf|auth|signature|credential/iu;
/** Query names whose values are credentials or one-time codes. */
const SECRET_PARAM =
  /token|key|secret|password|passwd|pwd|auth|session|sig|signature|code|credential|ticket|nonce/iu;

const record = (value: unknown): Params =>
  typeof value === 'object' && value !== null ? (value as Params) : {};
const short = (value: string) => (value.length > LONGEST ? `${value.slice(0, LONGEST)}…` : value);

/** The address with its user name, password, secret query values and fragment values hidden. */
export function redactUrl(text: string): string {
  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return short(text);
  }
  if (url.protocol === 'data:') return 'data:…';
  const user = url.username || url.password ? `${HIDDEN}@` : '';
  const query = [...url.searchParams]
    .map(([name, value]) =>
      SECRET_PARAM.test(name) && value !== '' ? `${name}=${HIDDEN}` : `${name}=${value}`,
    )
    .join('&');
  const fragment = url.hash.includes('=') ? `#${HIDDEN}` : url.hash;
  const host = `${url.protocol}//${user}${url.host}`;
  return `${url.protocol === 'file:' ? 'file://' : host}${url.pathname}${query ? `?${query}` : ''}${fragment}`;
}

function headerLines(value: unknown): string[] {
  return Object.entries(record(value))
    .slice(0, HEADERS)
    .map(([name, raw]) => {
      const text = typeof raw === 'string' ? raw : String(raw);
      return `${name}: ${SECRET_HEADER.test(name) ? HIDDEN : short(text)}`;
    });
}

export class NetworkLog {
  #entries: BrowserNetworkEntryV1[] = [];
  readonly #open = new Map<string, BrowserNetworkEntryV1>();

  /** Takes one DevTools Network event; false when it is not one. */
  message(method: string, params: Params): boolean {
    const id = typeof params.requestId === 'string' ? params.requestId : '';
    switch (method) {
      case 'Network.requestWillBeSent': {
        // A redirect ends the previous hop under the same id.
        const redirect = record(params.redirectResponse);
        const before = this.#open.get(id);
        if (before !== undefined && typeof redirect.status === 'number') {
          before.status = redirect.status;
          before.response_headers = headerLines(redirect.headers);
          this.#open.delete(id);
        }
        const request = record(params.request);
        const entry: BrowserNetworkEntryV1 = {
          method: String(request.method ?? 'GET'),
          url: redactUrl(String(request.url ?? '')),
          kind: String(params.type ?? 'other').toLowerCase(),
          status: null,
          mime_type: null,
          failure: null,
          request_headers: headerLines(request.headers),
          response_headers: [],
        };
        this.#push(entry);
        if (id) this.#open.set(id, entry);
        return true;
      }
      case 'Network.webSocketCreated':
        this.#push({
          method: 'GET',
          url: redactUrl(String(params.url ?? '')),
          kind: 'websocket',
          status: null,
          mime_type: null,
          failure: null,
          request_headers: [],
          response_headers: [],
        });
        return true;
      case 'Network.responseReceived': {
        const entry = this.#open.get(id);
        const response = record(params.response);
        if (entry !== undefined) {
          entry.status = typeof response.status === 'number' ? response.status : null;
          entry.mime_type = typeof response.mimeType === 'string' ? response.mimeType : null;
          entry.response_headers = headerLines(response.headers);
        }
        return true;
      }
      case 'Network.loadingFailed': {
        const entry = this.#open.get(id);
        if (entry !== undefined) {
          const blocked = params.errorText === 'net::ERR_BLOCKED_BY_CLIENT';
          entry.failure =
            params.canceled === true
              ? 'canceled'
              : `${String(params.errorText ?? 'failed')}${blocked ? '（浏览器设置不允许）' : ''}`;
        }
        this.#open.delete(id);
        return true;
      }
      case 'Network.loadingFinished':
        this.#open.delete(id);
        return true;
      default:
        return false;
    }
  }

  /** The newest `max` requests, newest last; only failed ones (or 400 and above) when asked. */
  entries(
    failedOnly: boolean,
    max: number,
  ): { entries: BrowserNetworkEntryV1[]; truncated: boolean } {
    const wanted = failedOnly
      ? this.#entries.filter(
          (entry) =>
            (entry.failure !== null && entry.failure !== 'canceled') || (entry.status ?? 0) >= 400,
        )
      : this.#entries;
    return {
      entries: wanted.slice(-max).map((entry) => ({ ...entry })),
      truncated: wanted.length > max,
    };
  }

  #push(entry: BrowserNetworkEntryV1): void {
    this.#entries.push(entry);
    if (this.#entries.length > KEEP) this.#entries = this.#entries.slice(-KEEP);
  }
}
