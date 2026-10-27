import type { BrowserBounds, BrowserUserCommand } from '../../shared/browser-api.js';

/** The renderer is untrusted; Main takes its browser commands only in their exact shape. */
const ID = /^[A-Za-z0-9_-]{1,80}$/u;
const PLAIN = new Set([
  'open',
  'back',
  'forward',
  'reload',
  'stop',
  'hide',
  'close',
  'take-control',
  'release-control',
  'dismiss-notice',
  'assist-done',
  'assist-decline',
  'devtools',
  'picture',
]);
const MAX_INPUT = 4096;
const MAX_EDGE = 20_000;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

function exactKeys(value: Record<string, unknown>, keys: readonly string[]): boolean {
  const own = Object.keys(value);
  return own.length === keys.length && keys.every((key) => Object.hasOwn(value, key));
}

function bounds(value: unknown): BrowserBounds | undefined {
  if (!isRecord(value) || !exactKeys(value, ['x', 'y', 'width', 'height'])) return undefined;
  const { x, y, width, height } = value;
  const edge = (n: unknown, min: number): n is number =>
    typeof n === 'number' && Number.isFinite(n) && n >= min && n <= MAX_EDGE;
  return edge(x, 0) && edge(y, 0) && edge(width, 1) && edge(height, 1)
    ? { x, y, width, height }
    : undefined;
}

export function parseUserCommand(value: unknown): BrowserUserCommand | undefined {
  if (!isRecord(value) || typeof value.kind !== 'string') return undefined;
  if (value.kind === 'clear-data')
    return exactKeys(value, ['kind']) ? { kind: 'clear-data' } : undefined;
  const { kind, sessionId, pageId } = value;
  if (typeof sessionId !== 'string' || typeof pageId !== 'string') return undefined;
  if (!ID.test(sessionId) || !ID.test(pageId)) return undefined;
  if (PLAIN.has(kind)) {
    return exactKeys(value, ['kind', 'sessionId', 'pageId'])
      ? ({ kind, sessionId, pageId } as BrowserUserCommand)
      : undefined;
  }
  if (kind === 'navigate') {
    const { input } = value;
    return exactKeys(value, ['kind', 'sessionId', 'pageId', 'input']) &&
      typeof input === 'string' &&
      input.length <= MAX_INPUT
      ? { kind, sessionId, pageId, input }
      : undefined;
  }
  if (kind === 'open-file') {
    const { workspaceId, path } = value;
    return exactKeys(value, ['kind', 'sessionId', 'pageId', 'workspaceId', 'path']) &&
      typeof workspaceId === 'string' &&
      ID.test(workspaceId) &&
      typeof path === 'string' &&
      path.length > 0 &&
      path.length <= MAX_INPUT
      ? { kind, sessionId, pageId, workspaceId, path }
      : undefined;
  }
  if (kind === 'show') {
    const parsed = bounds(value.bounds);
    return exactKeys(value, ['kind', 'sessionId', 'pageId', 'bounds']) && parsed !== undefined
      ? { kind, sessionId, pageId, bounds: parsed }
      : undefined;
  }
  return undefined;
}
