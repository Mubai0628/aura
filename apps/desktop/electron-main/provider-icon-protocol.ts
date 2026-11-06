import { Buffer } from 'node:buffer';
import type { ProviderIconReadRequest, ProviderIconReadResult } from '@aura/contracts/generated';

/** One `provider_icon.read` through the Core supervisor. */
export type ProviderIconRead = (params: ProviderIconReadRequest) => Promise<ProviderIconReadResult>;

/**
 * Core reads in flight for provider icons. Core runs eight read-only requests at once and queues 32
 * more per connection, refusing the rest; a catalog asks for an icon per provider, so icons go a few
 * at a time and leave that lane to the reads behind them.
 */
export const MAX_PROVIDER_ICON_READS = 4;

/** Icons kept in memory, by bytes. An icon is a few KiB, so this holds every catalog's icons. */
const CACHE_BYTES = 4 * 1024 * 1024;

/** An icon's PNG bytes, decoded into memory of their own. */
type Icon = Buffer<ArrayBuffer>;

const PNG_SIGNATURE = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const ADDRESS = /^\/provider-icon\/(art_[0-7][0-9A-HJKMNP-TV-Z]{25})\/([0-9a-f]{64})\.png$/u;

/**
 * `aura://app/provider-icon/<artifact>/<hash>.png`: the provider icons Core validated, read on
 * demand, a few at a time, and kept by content hash. An address names one immutable icon, so a kept
 * icon never goes stale; a failed read is not kept, so the next showing asks Core again.
 */
export class ProviderIconProtocol {
  readonly #read: ProviderIconRead;
  readonly #maxReads: number;
  readonly #cacheBytes: number;
  /** Least recently shown first: a hit moves its icon to the end. */
  readonly #kept = new Map<string, Icon>();
  readonly #reading = new Map<string, Promise<Icon | undefined>>();
  readonly #waiting: (() => void)[] = [];
  #running = 0;
  #keptBytes = 0;

  constructor(read: ProviderIconRead, options: { cacheBytes?: number; maxReads?: number } = {}) {
    this.#read = read;
    this.#maxReads = options.maxReads ?? MAX_PROVIDER_ICON_READS;
    this.#cacheBytes = options.cacheBytes ?? CACHE_BYTES;
  }

  /** Bytes of the icons kept now. */
  get cachedBytes(): number {
    return this.#keptBytes;
  }

  /** The icon for `pathname`, or `undefined` when the address is not a provider icon's. */
  async serve(pathname: string): Promise<Response | undefined> {
    const match = ADDRESS.exec(pathname);
    if (match?.[1] === undefined || match[2] === undefined) return undefined;
    const key = `${match[1]}/${match[2]}`;
    const kept = this.#kept.get(key);
    if (kept !== undefined) {
      this.#kept.delete(key);
      this.#kept.set(key, kept);
      return iconResponse(kept);
    }
    let reading = this.#reading.get(key);
    if (reading === undefined) {
      reading = this.#readOnce(match[1] as `art_${string}`, match[2]).finally(() =>
        this.#reading.delete(key),
      );
      this.#reading.set(key, reading);
    }
    const bytes = await reading;
    if (bytes === undefined) return new Response('Not found', { status: 404 });
    this.#keep(key, bytes);
    return iconResponse(bytes);
  }

  async #readOnce(artifactId: `art_${string}`, hex: string): Promise<Icon | undefined> {
    // A finished read hands its place straight to the next waiter, so an icon asked for in between
    // cannot take that place as well.
    if (this.#running >= this.#maxReads) {
      await new Promise<void>((start) => this.#waiting.push(start));
    } else {
      this.#running += 1;
    }
    try {
      const result = await this.#read({
        artifact_id: artifactId,
        content_hash: { algorithm: 'blake3', hex },
      });
      const bytes = Buffer.from(result.png_base64, 'base64');
      return bytes.byteLength === result.byte_length &&
        result.content_hash.algorithm === 'blake3' &&
        result.content_hash.hex === hex &&
        bytes.subarray(0, 8).equals(PNG_SIGNATURE)
        ? bytes
        : undefined;
    } catch {
      return undefined;
    } finally {
      const next = this.#waiting.shift();
      if (next === undefined) this.#running -= 1;
      else next();
    }
  }

  #keep(key: string, bytes: Icon): void {
    if (this.#kept.has(key) || bytes.byteLength > this.#cacheBytes) return;
    this.#kept.set(key, bytes);
    this.#keptBytes += bytes.byteLength;
    for (const [oldest, oldBytes] of this.#kept) {
      if (this.#keptBytes <= this.#cacheBytes) break;
      this.#kept.delete(oldest);
      this.#keptBytes -= oldBytes.byteLength;
    }
  }
}

function iconResponse(bytes: Icon): Response {
  return new Response(bytes, {
    headers: {
      'Cache-Control': 'private, max-age=86400, immutable',
      'Content-Length': String(bytes.byteLength),
      'Content-Type': 'image/png',
      'X-Content-Type-Options': 'nosniff',
    },
    status: 200,
  });
}
