/**
 * What the address bar opens, and whether a host is this computer, the local network or the
 * internet. The class is read from the address as written; a public name that resolves to a
 * private address is not caught here.
 */
export type HostClass = 'loopback' | 'private' | 'public';

function tryUrl(text: string): URL | undefined {
  try {
    return new URL(text);
  } catch {
    return undefined;
  }
}

function ipv4Class(parts: readonly number[]): HostClass {
  const [a = 0, b = 0] = parts;
  if (a === 127 || a === 0) return 'loopback';
  if (
    a === 10 ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 169 && b === 254) ||
    (a === 100 && b >= 64 && b <= 127)
  )
    return 'private';
  return 'public';
}

function ipv4(text: string): number[] | undefined {
  const match = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/u.exec(text);
  if (match === null) return undefined;
  const parts = match.slice(1).map(Number);
  return parts.every((part) => part <= 255) ? parts : undefined;
}

function ipv6Class(text: string): HostClass {
  const value = text.toLowerCase();
  if (value === '::1' || value === '::') return 'loopback';
  const mapped = /^::ffff:(.+)$/u.exec(value)?.[1];
  if (mapped !== undefined) {
    const dotted = ipv4(mapped);
    if (dotted !== undefined) return ipv4Class(dotted);
    const hex = /^([0-9a-f]{1,4}):([0-9a-f]{1,4})$/u.exec(mapped);
    if (hex?.[1] !== undefined && hex[2] !== undefined) {
      const high = parseInt(hex[1], 16);
      const low = parseInt(hex[2], 16);
      return ipv4Class([high >> 8, high & 255, low >> 8, low & 255]);
    }
  }
  const first = value.startsWith('::') ? 0 : parseInt(value.split(':')[0] ?? '0', 16);
  if ((first & 0xfe00) === 0xfc00 || (first & 0xffc0) === 0xfe80) return 'private';
  return 'public';
}

export function hostClass(host: string): HostClass {
  const value = host.toLowerCase().replace(/\.+$/u, '');
  if (value === 'localhost' || value.endsWith('.localhost')) return 'loopback';
  if (value.startsWith('[') && value.endsWith(']')) return ipv6Class(value.slice(1, -1));
  const dotted = ipv4(value);
  return dotted === undefined ? 'public' : ipv4Class(dotted);
}

/**
 * The page the address bar opens: an http(s) address as written, a host (https, or http for this
 * computer and the local network), or about:blank. Anything else is not opened.
 */
export function addressFromInput(text: string): string | undefined {
  const value = text.trim();
  if (value === '' || /\s/u.test(value)) return undefined;
  if (/^about:blank$/iu.test(value)) return 'about:blank';
  if (/^[a-z][a-z0-9+.-]*:\/\//iu.test(value)) {
    const url = tryUrl(value);
    return url !== undefined && (url.protocol === 'http:' || url.protocol === 'https:')
      ? url.href
      : undefined;
  }
  // "javascript:…", "mailto:…" — a scheme without "//"; "localhost:5173" is a host and port.
  if (/^[a-z][a-z0-9+.-]*:(?!\d)/iu.test(value)) return undefined;
  const guess = tryUrl(`http://${value}`);
  if (guess === undefined) return undefined;
  if (hostClass(guess.hostname) !== 'public') return guess.href;
  if (!guess.hostname.includes('.')) return undefined;
  return tryUrl(`https://${value}`)?.href;
}
