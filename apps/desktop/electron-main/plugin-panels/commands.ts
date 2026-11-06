import type { BrowserBounds } from '../../shared/browser-api.js';
import type { PluginPanelCommand } from '../../shared/plugin-panel-api.js';
import type { PanelSite } from './site.js';

/** The renderer is untrusted; Main takes its panel commands only in their exact shape. */
const SESSION = /^ses_[0-9A-HJKMNP-TV-Z]{26}$/u;
/** A plugin's name, as its manifest allows it. */
export const PLUGIN = /^[a-z][a-z0-9-]{0,31}$/u;
const PLAIN = new Set(['open', 'hide', 'close', 'picture']);
const MAX_EDGE = 20_000;
const MAX_TEXT = 4_096;

const record = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const exactKeys = (value: Record<string, unknown>, keys: readonly string[]) =>
  Object.keys(value).length === keys.length && keys.every((key) => Object.hasOwn(value, key));

function bounds(value: unknown): BrowserBounds | undefined {
  if (!record(value) || !exactKeys(value, ['x', 'y', 'width', 'height'])) return undefined;
  const { x, y, width, height } = value;
  const edge = (n: unknown, min: number): n is number =>
    typeof n === 'number' && Number.isFinite(n) && n >= min && n <= MAX_EDGE;
  return edge(x, 0) && edge(y, 0) && edge(width, 1) && edge(height, 1)
    ? { x, y, width, height }
    : undefined;
}

export function readPanelCommand(value: unknown): PluginPanelCommand | undefined {
  if (!record(value) || typeof value.kind !== 'string') return undefined;
  const { kind, sessionId, plugin } = value;
  if (typeof sessionId !== 'string' || !SESSION.test(sessionId)) return undefined;
  if (typeof plugin !== 'string' || !PLUGIN.test(plugin)) return undefined;
  if (PLAIN.has(kind))
    return exactKeys(value, ['kind', 'sessionId', 'plugin'])
      ? ({ kind, sessionId, plugin } as PluginPanelCommand)
      : undefined;
  if (kind === 'show') {
    const parsed = bounds(value.bounds);
    return exactKeys(value, ['kind', 'sessionId', 'plugin', 'bounds']) && parsed !== undefined
      ? { kind, sessionId, plugin, bounds: parsed }
      : undefined;
  }
  return undefined;
}

const strings = (value: unknown, max: number): value is string[] =>
  Array.isArray(value) &&
  value.length <= max &&
  value.every((item) => typeof item === 'string' && item.length <= MAX_TEXT);

/** Core's answer for `agent.plugin_session.panel`, in exactly its shape, for `plugin`. */
export function readPanelSite(value: unknown, plugin: string): PanelSite | undefined {
  if (
    !record(value) ||
    !exactKeys(value, ['title', 'folder', 'entry', 'network', 'capabilities']) ||
    typeof value.title !== 'string' ||
    typeof value.folder !== 'string' ||
    value.folder === '' ||
    typeof value.entry !== 'string' ||
    !value.entry.startsWith('panel/') ||
    !strings(value.network, 64) ||
    !value.network.every((origin) =>
      /^(?:https|wss):\/\/[a-z0-9.-]+(?::\d{1,5})?\/?$/iu.test(origin),
    ) ||
    !strings(value.capabilities, 32)
  )
    return undefined;
  return {
    plugin,
    title: value.title,
    folder: value.folder,
    entry: value.entry,
    network: value.network,
    capabilities: value.capabilities,
  };
}

/** A refusal for the user to read: Core's message without its code. */
export function shownMessage(error: unknown, fallback: string): string {
  const envelope = record(error) && record(error.envelope) ? error.envelope : undefined;
  const message =
    typeof envelope?.message === 'string'
      ? envelope.message
      : error instanceof Error
        ? error.message
        : '';
  const plain = message.replace(/^[a-z][a-z0-9_]*\.[a-z][a-z0-9_.]*:\s*/u, '').trim();
  return plain === '' ? fallback : plain.slice(0, 500);
}
