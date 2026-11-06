/**
 * What plugins show in Aura's own window: notices, and one status entry each in the plugin's row
 * on the 插件 settings page.
 * Main gathers them from the plugins' panels (and their background services) after Core checked
 * each; the window only shows what Main sends.
 */
export const PLUGIN_UI_CHANGE_CHANNEL = 'aura:v1:plugin-ui-change';
export const PLUGIN_UI_STATUSES_CHANNEL = 'aura:v1:plugin-ui-statuses';

export type PluginUiTone = 'info' | 'success' | 'warning' | 'error';

/** A plugin's status entry, as it is shown now. */
export interface PluginStatusEntry {
  plugin: string;
  text: string;
  tone: PluginUiTone;
}

/** A notice a plugin showed. */
export interface PluginNoticeShown {
  plugin: string;
  text: string;
  tone: PluginUiTone;
}

export type PluginUiChange =
  | { kind: 'statuses'; statuses: PluginStatusEntry[] }
  | { kind: 'notice'; notice: PluginNoticeShown }
  /** A plugin's background service changed state. */
  | { kind: 'service'; plugin: string };

export interface PluginUiApiV1 {
  /** The status entries shown now. */
  statuses(): Promise<PluginStatusEntry[]>;
  onChange(listener: (change: PluginUiChange) => void): () => void;
}

/** The most entries the window shows, and how long a line may be by the time it gets there. */
export const PLUGIN_STATUS_ENTRIES_MAX = 16;
const MAX_TEXT = 200;
const TONES: ReadonlySet<string> = new Set(['info', 'success', 'warning', 'error']);

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const isPluginName = (value: unknown): value is string =>
  typeof value === 'string' && /^[a-z][a-z0-9-]{0,31}$/u.test(value);

function readItem(value: unknown): PluginStatusEntry | undefined {
  if (!isRecord(value)) return undefined;
  const { plugin, text, tone } = value;
  return isPluginName(plugin) &&
    typeof text === 'string' &&
    text !== '' &&
    typeof tone === 'string' &&
    TONES.has(tone)
    ? { plugin, text: text.slice(0, MAX_TEXT), tone: tone as PluginUiTone }
    : undefined;
}

/** Main's status entries, in exactly their shape; anything else shows none. */
export function readPluginStatuses(value: unknown): PluginStatusEntry[] {
  if (!Array.isArray(value)) return [];
  return value.slice(0, PLUGIN_STATUS_ENTRIES_MAX).flatMap((item) => {
    const entry = readItem(item);
    return entry === undefined ? [] : [entry];
  });
}

/** A change from Main, in exactly its shape. */
export function readPluginUiChange(value: unknown): PluginUiChange | undefined {
  if (!isRecord(value)) return undefined;
  if (value.kind === 'statuses' && Array.isArray(value.statuses))
    return { kind: 'statuses', statuses: readPluginStatuses(value.statuses) };
  if (value.kind === 'notice') {
    const notice = readItem(value.notice);
    return notice === undefined ? undefined : { kind: 'notice', notice };
  }
  if (value.kind === 'service' && isPluginName(value.plugin))
    return { kind: 'service', plugin: value.plugin };
  return undefined;
}
