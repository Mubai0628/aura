/**
 * The preload of a plugin's panel page. It gives the page `window.auraPanel` with only the calls
 * the plugin declared. Each call goes to Main, which knows which panel asked and acts in that
 * panel's conversation; the page never names a plugin or a conversation.
 */
import { contextBridge, ipcRenderer } from 'electron';
import {
  PANEL_CALL_CAPABILITIES,
  PLUGIN_PANEL_CALL_CHANNEL,
  panelCallValue,
  readPanelCapabilities,
  type PanelCall,
} from '../shared/plugin-panel-api.js';
import type { PluginUiTone } from '../shared/plugin-ui-api.js';

const declared = readPanelCapabilities(process.argv);

async function call(request: PanelCall): Promise<unknown> {
  return panelCallValue(await ipcRenderer.invoke(PLUGIN_PANEL_CALL_CHANNEL, request));
}

const may = (op: PanelCall['op']) => declared.has(PANEL_CALL_CAPABILITIES[op]);
const panel: Record<string, unknown> = {
  version: 1,
  capabilities: Object.freeze([...declared]),
};
// Main checks every value again; these only shape the call.
if (may('state')) panel.state = () => call({ op: 'state' });
if (may('events'))
  panel.events = (after?: unknown) =>
    call(after === undefined ? { op: 'events' } : { op: 'events', after: after as number });
if (may('send'))
  panel.send = (text: unknown, options?: unknown) => {
    const key =
      typeof options === 'object' && options !== null && 'dedupeKey' in options
        ? (options as { dedupeKey?: unknown }).dedupeKey
        : undefined;
    return call(
      key === undefined
        ? { op: 'send', text: text as string }
        : { op: 'send', text: text as string, dedupe_key: key as string },
    );
  };
if (may('run'))
  panel.run = (name: unknown, options?: unknown) => {
    const given = typeof options === 'object' && options !== null ? options : {};
    const { arguments: text, dedupeKey } = given as { arguments?: unknown; dedupeKey?: unknown };
    return call({
      op: 'run',
      name: name as string,
      ...(text === undefined ? {} : { arguments: text as string }),
      ...(dedupeKey === undefined ? {} : { dedupe_key: dedupeKey as string }),
    });
  };
if (may('stop')) panel.stop = () => call({ op: 'stop' });
if (may('settings')) {
  panel.settings = () => call({ op: 'settings' });
  panel.setSetting = (key: unknown, value: unknown) =>
    call({ op: 'set_setting', key: key as string, value: value as string });
}
const toneOf = (options: unknown) =>
  typeof options === 'object' && options !== null && 'tone' in options
    ? ((options as { tone?: unknown }).tone as PluginUiTone)
    : undefined;
if (may('notify'))
  panel.notify = (text: unknown, options?: unknown) => {
    const tone = toneOf(options);
    return call(
      tone === undefined
        ? { op: 'notify', text: text as string }
        : { op: 'notify', text: text as string, tone },
    );
  };
if (may('status'))
  panel.status = (text?: unknown, options?: unknown) => {
    const tone = toneOf(options);
    const shown = text === undefined || text === null ? {} : { text: text as string };
    return call(tone === undefined ? { op: 'status', ...shown } : { op: 'status', ...shown, tone });
  };

contextBridge.exposeInMainWorld('auraPanel', Object.freeze(panel));
