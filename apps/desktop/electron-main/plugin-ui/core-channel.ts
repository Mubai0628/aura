/**
 * What plugins' background services show, from Core as the `plugin.ui.v1` notification once
 * Core checked it. Each change is taken only in exactly its contract shape; anything else is
 * dropped. A service's entries belong to the Core that ran it: a new Core starts with none.
 */
import type { PluginServiceUi } from '@aura/contracts/generated';
import type { PluginUiTone } from '../../shared/plugin-ui-api.js';
import type { PluginUiHub } from './hub.js';

/** The source a service's status entry is held under in the hub. */
export const SERVICE_SOURCE = 'service';
const TONES: ReadonlySet<string> = new Set(['info', 'success', 'warning', 'error']);
const MAX_TEXT = 200;

type Fields = Record<string, unknown>;
const isRecord = (value: unknown): value is Fields =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const exactly = (value: Fields, keys: readonly string[]) =>
  Object.keys(value).every((key) => keys.includes(key));
const isPlugin = (value: unknown): value is string =>
  typeof value === 'string' && /^[a-z][a-z0-9-]{0,31}$/u.test(value);
const isText = (value: unknown): value is string =>
  typeof value === 'string' && value !== '' && [...value].length <= MAX_TEXT;
const isTone = (value: unknown): value is PluginUiTone =>
  typeof value === 'string' && TONES.has(value);

/** One `plugin.ui.v1` change, in exactly its shape. */
export function readServiceUi(value: unknown): PluginServiceUi | undefined {
  if (!isRecord(value)) return undefined;
  const { kind } = value;
  if (kind === 'notice' && exactly(value, ['kind', 'notice'])) {
    const notice = value.notice;
    if (!isRecord(notice) || !exactly(notice, ['plugin', 'text', 'tone'])) return undefined;
    const { plugin, text, tone } = notice;
    return isPlugin(plugin) && isText(text) && isTone(tone)
      ? { kind, notice: { plugin, text, tone } }
      : undefined;
  }
  if (kind === 'status' && exactly(value, ['kind', 'status'])) {
    const status = value.status;
    if (!isRecord(status) || !exactly(status, ['plugin', 'text', 'tone'])) return undefined;
    const { plugin, text, tone } = status;
    if (!isPlugin(plugin) || !isTone(tone)) return undefined;
    if (text === undefined || text === null) return { kind, status: { plugin, tone } };
    return isText(text) ? { kind, status: { plugin, text, tone } } : undefined;
  }
  if ((kind === 'ended' || kind === 'service') && exactly(value, ['kind', 'plugin']))
    return isPlugin(value.plugin) ? { kind, plugin: value.plugin } : undefined;
  return undefined;
}

let hub: PluginUiHub | undefined;
let runtime: string | undefined;

/** The hub the services' changes go to. */
export function connectServiceUi(target: PluginUiHub): void {
  hub = target;
  runtime = undefined;
}

/** True when the notification was a service's change (applied, or dropped as malformed). */
export function pluginUiNotification(method: string, params: unknown, runtimeId: string): boolean {
  if (method !== 'plugin.ui.v1') return false;
  const change = readServiceUi(params);
  if (change === undefined || hub === undefined) return true;
  if (runtime !== runtimeId) {
    if (runtime !== undefined) hub.dropSource(SERVICE_SOURCE);
    runtime = runtimeId;
  }
  switch (change.kind) {
    case 'notice':
      hub.notice(change.notice);
      break;
    case 'status':
      hub.status(
        change.status.plugin,
        SERVICE_SOURCE,
        change.status.text ?? undefined,
        change.status.tone,
      );
      break;
    case 'ended':
      hub.drop(change.plugin, SERVICE_SOURCE);
      break;
    case 'service':
      hub.serviceChanged(change.plugin);
      break;
  }
  return true;
}
