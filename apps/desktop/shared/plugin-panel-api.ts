import type { BrowserBounds } from './browser-api.js';
import type { PluginUiTone } from './plugin-ui-api.js';

/**
 * A plugin's panel: a page of the plugin's own files in the right sidebar, in a sandbox of its
 * own. The renderer places it; the page talks to Aura only through `window.auraPanel`, and every
 * call acts in the conversation the panel was opened in.
 */
export const PLUGIN_PANEL_COMMAND_CHANNEL = 'aura:v1:plugin-panel-command';
export const PLUGIN_PANEL_STATE_CHANNEL = 'aura:v1:plugin-panel-state';
/** The panel page's own channel; Main answers it only from a panel page it made. */
export const PLUGIN_PANEL_CALL_CHANNEL = 'aura:v1:plugin-panel-call';
/** How Main tells a panel's preload what the plugin declared. */
export const PANEL_CAPABILITIES_ARGUMENT = '--aura-panel-capabilities=';

export type PluginPanelCommand =
  | {
      kind: 'open' | 'hide' | 'close' | 'picture';
      sessionId: string;
      plugin: string;
    }
  | { kind: 'show'; sessionId: string; plugin: string; bounds: BrowserBounds };

export type PluginPanelResult =
  | {
      ok: true;
      /** For 'picture': a JPEG data URL; absent when the page could not be pictured. */
      picture?: string | undefined;
    }
  | { ok: false; message: string };

/** A panel that stopped (its page crashed, failed to load, or its plugin can no longer be used). */
export interface PluginPanelState {
  sessionId: string;
  plugin: string;
  stopped: string;
}

export interface PluginPanelsApiV1 {
  command(command: PluginPanelCommand): Promise<PluginPanelResult>;
  onState(listener: (state: PluginPanelState) => void): () => void;
}

/**
 * What a panel page asks for; Main fills in the plugin and the conversation. A notice, the status
 * entry and the plugin's settings are the plugin's own and belong to no conversation.
 */
export type PanelCall =
  | { op: 'state' }
  | { op: 'events'; after?: number }
  | { op: 'send'; text: string; dedupe_key?: string }
  | { op: 'run'; name: string; arguments?: string; dedupe_key?: string }
  | { op: 'stop' }
  | { op: 'notify'; text: string; tone?: PluginUiTone }
  | { op: 'status'; text?: string; tone?: PluginUiTone }
  | { op: 'settings' }
  | { op: 'set_setting'; key: string; value: string };

export type PanelCallResult = { ok: true; value: unknown } | { ok: false; message: string };

/** Each call and the capability its plugin must have declared. */
export const PANEL_CALL_CAPABILITIES: Record<PanelCall['op'], string> = {
  state: 'session.read',
  events: 'session.events',
  send: 'session.send',
  run: 'commands',
  stop: 'session.control',
  notify: 'notify',
  status: 'status',
  settings: 'settings',
  set_setting: 'settings',
};

/** The most characters of a message a panel sends, and of a dedupe key (Core holds the same). */
export const PANEL_MESSAGE_MAX_CHARS = 16_000;
export const PANEL_DEDUPE_KEY_MAX_CHARS = 128;
/** The longest notice and status entry (Core holds the same). */
export const PANEL_NOTICE_MAX_CHARS = 200;
export const PANEL_STATUS_MAX_CHARS = 40;
/** The longest value of a text setting (Core holds the same). */
export const PANEL_SETTING_MAX_CHARS = 2_000;

/** A panel's picture is only ever a JPEG Main encoded. */
const PICTURE_PREFIX = 'data:image/jpeg;base64,';
const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

/** Main's answer to a panel command, in exactly its shape. */
export function readPanelResult(value: unknown): PluginPanelResult {
  if (isRecord(value) && value.ok === false && typeof value.message === 'string')
    return { ok: false, message: value.message.slice(0, 1_000) };
  if (isRecord(value) && value.ok === true)
    return {
      ok: true,
      picture:
        typeof value.picture === 'string' && value.picture.startsWith(PICTURE_PREFIX)
          ? value.picture
          : undefined,
    };
  return { ok: false, message: '插件面板没有回应' };
}

/** A stopped panel from Main, in exactly its shape. */
export function readPanelState(value: unknown): PluginPanelState | undefined {
  return isRecord(value) &&
    typeof value.sessionId === 'string' &&
    typeof value.plugin === 'string' &&
    typeof value.stopped === 'string'
    ? { sessionId: value.sessionId, plugin: value.plugin, stopped: value.stopped.slice(0, 1_000) }
    : undefined;
}

/** The capabilities Main gave a panel page's preload in its arguments. */
export function readPanelCapabilities(argv: readonly string[]): ReadonlySet<string> {
  const given = argv.find((value) => value.startsWith(PANEL_CAPABILITIES_ARGUMENT));
  return new Set(
    (given?.slice(PANEL_CAPABILITIES_ARGUMENT.length) ?? '')
      .split(',')
      .filter((name) => name !== ''),
  );
}

/** What a panel page gets from a call: Main's value, or its refusal thrown as an error. */
export function panelCallValue(answer: unknown): unknown {
  if (isRecord(answer) && answer.ok === true && 'value' in answer) return answer.value;
  if (isRecord(answer) && answer.ok === false && typeof answer.message === 'string')
    throw new Error(answer.message);
  throw new Error('plugin_panel.failed: Aura 没有回应');
}
