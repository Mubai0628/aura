/**
 * What a panel page may ask through `window.auraPanel`, checked in exactly its shape before Main
 * asks Core. The plugin and the conversation are always the panel's own; the page never names
 * them. Core checks the capability again against the plugin's trusted manifest.
 */
import {
  PANEL_CALL_CAPABILITIES,
  PANEL_DEDUPE_KEY_MAX_CHARS,
  PANEL_MESSAGE_MAX_CHARS,
  PANEL_NOTICE_MAX_CHARS,
  PANEL_SETTING_MAX_CHARS,
  PANEL_STATUS_MAX_CHARS,
  type PanelCall,
} from '../../shared/plugin-panel-api.js';
import type { PluginUiTone } from '../../shared/plugin-ui-api.js';

const TONES: ReadonlySet<unknown> = new Set(['info', 'success', 'warning', 'error']);
/** A command's name and a setting's key, as the plugin's manifest and files may name them. */
const COMMAND_NAME = /^[a-z][a-z0-9-]{0,39}$/u;
const SETTING_KEY = /^[a-z][a-z0-9_]{0,39}$/u;
/** The calls that change something for the user, held to fewer than the rest. */
const CHANGING: ReadonlySet<PanelCall['op']> = new Set(['send', 'run', 'set_setting']);

const record = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const only = (value: Record<string, unknown>, ...keys: string[]) =>
  Object.keys(value).every((key) => keys.includes(key));
const text = (value: unknown, max: number): value is string =>
  typeof value === 'string' && value.trim() !== '' && [...value].length <= max;
const upTo = (value: unknown, max: number): value is string =>
  typeof value === 'string' && [...value].length <= max;
const dedupeKey = (value: unknown): value is string =>
  text(value, PANEL_DEDUPE_KEY_MAX_CHARS) &&
  // eslint-disable-next-line no-control-regex -- a key is one line of text
  !/[\u0000-\u001f\u007f]/u.test(value);

/** The call, or undefined when it is not one a panel may make. */
export function readPanelCall(value: unknown): PanelCall | undefined {
  if (!record(value)) return undefined;
  switch (value.op) {
    case 'state':
    case 'stop':
    case 'settings':
      return only(value, 'op') ? { op: value.op } : undefined;
    case 'events': {
      if (!only(value, 'op', 'after')) return undefined;
      if (value.after === undefined) return { op: 'events' };
      return typeof value.after === 'number' &&
        Number.isSafeInteger(value.after) &&
        value.after >= 0
        ? { op: 'events', after: value.after }
        : undefined;
    }
    case 'send': {
      if (!only(value, 'op', 'text', 'dedupe_key')) return undefined;
      if (!text(value.text, PANEL_MESSAGE_MAX_CHARS)) return undefined;
      if (value.dedupe_key === undefined) return { op: 'send', text: value.text };
      return dedupeKey(value.dedupe_key)
        ? { op: 'send', text: value.text, dedupe_key: value.dedupe_key }
        : undefined;
    }
    case 'run': {
      if (!only(value, 'op', 'name', 'arguments', 'dedupe_key')) return undefined;
      if (typeof value.name !== 'string' || !COMMAND_NAME.test(value.name)) return undefined;
      if (value.arguments !== undefined && !upTo(value.arguments, PANEL_MESSAGE_MAX_CHARS))
        return undefined;
      if (value.dedupe_key !== undefined && !dedupeKey(value.dedupe_key)) return undefined;
      return {
        op: 'run',
        name: value.name,
        ...(value.arguments === undefined ? {} : { arguments: value.arguments }),
        ...(value.dedupe_key === undefined ? {} : { dedupe_key: value.dedupe_key }),
      };
    }
    case 'set_setting':
      // An empty value clears the setting; Core checks the value as the plugin page does.
      return only(value, 'op', 'key', 'value') &&
        typeof value.key === 'string' &&
        SETTING_KEY.test(value.key) &&
        upTo(value.value, PANEL_SETTING_MAX_CHARS)
        ? { op: 'set_setting', key: value.key, value: value.value }
        : undefined;
    case 'notify': {
      if (!only(value, 'op', 'text', 'tone') || !text(value.text, PANEL_NOTICE_MAX_CHARS))
        return undefined;
      if (value.tone === undefined) return { op: 'notify', text: value.text };
      return TONES.has(value.tone)
        ? { op: 'notify', text: value.text, tone: value.tone as PluginUiTone }
        : undefined;
    }
    case 'status': {
      if (!only(value, 'op', 'text', 'tone')) return undefined;
      if (value.tone !== undefined && !TONES.has(value.tone)) return undefined;
      const tone = value.tone as PluginUiTone | undefined;
      // No text, or a blank one, clears the entry.
      if (value.text === undefined || (typeof value.text === 'string' && value.text.trim() === ''))
        return tone === undefined ? { op: 'status' } : { op: 'status', tone };
      if (!text(value.text, PANEL_STATUS_MAX_CHARS)) return undefined;
      return tone === undefined
        ? { op: 'status', text: value.text }
        : { op: 'status', text: value.text, tone };
    }
    default:
      return undefined;
  }
}

/**
 * Core's route and parameters for the panel of `plugin` in `sessionId`. A notice, the status
 * entry and the settings are the plugin's own, so they name no conversation.
 */
export function coreCall(
  call: PanelCall,
  plugin: string,
  sessionId: string,
): { method: string; params: Record<string, unknown> } {
  const method = `agent.plugin_session.${call.op}`;
  if (call.op === 'notify' || call.op === 'status') {
    const params: Record<string, unknown> = { plugin };
    if (call.text !== undefined) params.text = call.text;
    if (call.tone !== undefined) params.tone = call.tone;
    return { method, params };
  }
  if (call.op === 'settings') return { method, params: { plugin } };
  if (call.op === 'set_setting')
    return { method, params: { plugin, key: call.key, value: call.value } };
  const params: Record<string, unknown> = { plugin, session_id: sessionId };
  if (call.op === 'events' && call.after !== undefined) params.after = call.after;
  if (call.op === 'send') params.text = call.text;
  if (call.op === 'run') {
    params.name = call.name;
    if (call.arguments !== undefined) params.arguments = call.arguments;
  }
  if ((call.op === 'send' || call.op === 'run') && call.dedupe_key !== undefined)
    params.dedupe_key = call.dedupe_key;
  return { method, params };
}

/** Core's checked notice or status entry, for this plugin, in exactly its shape. */
export function shownItem(
  value: unknown,
  plugin: string,
): { text: string | undefined; tone: PluginUiTone } | undefined {
  if (!record(value) || value.plugin !== plugin || !TONES.has(value.tone)) return undefined;
  if (value.text !== undefined && typeof value.text !== 'string') return undefined;
  return { text: value.text === '' ? undefined : value.text, tone: value.tone as PluginUiTone };
}

/** Whether the plugin declared what the call needs; Core checks it again. */
export function declared(call: PanelCall, capabilities: readonly string[]): boolean {
  return capabilities.includes(PANEL_CALL_CAPABILITIES[call.op]);
}

/**
 * How often one panel may call: a page that loops never floods Core. Messages it sends, commands
 * it runs and settings it sets are held to fewer still.
 */
export class CallBudget {
  readonly #calls: number[] = [];
  readonly #sends: number[] = [];

  constructor(
    private readonly limit = 40,
    private readonly sendLimit = 5,
    private readonly windowMs = 10_000,
    private readonly now: () => number = Date.now,
  ) {}

  /** Takes one call from the budget, or says it is spent. */
  take(op: PanelCall['op']): boolean {
    const now = this.now();
    for (const list of [this.#calls, this.#sends])
      while (list.length > 0 && now - list[0]! >= this.windowMs) list.shift();
    if (this.#calls.length >= this.limit) return false;
    const changing = CHANGING.has(op);
    if (changing && this.#sends.length >= this.sendLimit) return false;
    this.#calls.push(now);
    if (changing) this.#sends.push(now);
    return true;
  }
}
