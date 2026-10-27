import { describe, expect, it, vi } from 'vitest';
import { Background } from './background.js';
import type { ChromeApi, ChromeEvent, Port, Tab } from './chrome-api.js';
import { PANEL_PORT, type PanelState } from './panel-messages.js';
import type { CoreMessage } from './protocol.js';

function event<Listener extends (...args: never[]) => unknown>() {
  const listeners: Listener[] = [];
  const api: ChromeEvent<Listener> = {
    addListener: (listener) => void listeners.push(listener),
    removeListener: () => undefined,
  };
  return { api, fire: (...args: Parameters<Listener>) => listeners.forEach((l) => l(...args)) };
}

function port(name: string) {
  const messages = event<(message: unknown) => void>();
  const closes = event<() => void>();
  const sent: unknown[] = [];
  const value: Port = {
    name,
    postMessage: (message) => void sent.push(message),
    disconnect: () => undefined,
    onMessage: messages.api,
    onDisconnect: closes.api,
  };
  return { value, sent, receive: messages.fire, close: closes.fire };
}

const TAB: Tab = {
  id: 7,
  windowId: 1,
  url: 'https://shop.example/',
  title: 'Shop',
  active: true,
  groupId: -1,
  status: 'complete',
};

function harness() {
  const native = port('native');
  const connects = event<(port: Port) => void>();
  const store: Record<string, unknown> = { pairing_secret: 'AAAA' };
  const chrome: ChromeApi = {
    runtime: {
      id: 'bheiijocpacefhgapeegbnekchcpedkj',
      getManifest: () => ({ version: '0.1.3' }),
      connectNative: () => native.value,
      connect: () => native.value,
      onConnect: connects.api,
    },
    storage: {
      local: {
        get: async (key) => ({ [key]: store[key] }),
        set: async (items) => void Object.assign(store, items),
        remove: async (key) => void delete store[key],
      },
    },
    debugger: {
      attach: async () => undefined,
      detach: async () => undefined,
      sendCommand: async () => ({}),
      onEvent: event<() => void>().api,
      onDetach: event<() => void>().api,
    },
    tabs: {
      get: async () => TAB,
      query: async () => [TAB],
      create: async () => TAB,
      remove: async () => undefined,
      update: async () => TAB,
      group: async () => 1,
      ungroup: async () => undefined,
      onUpdated: event<() => void>().api,
      onRemoved: event<() => void>().api,
      onActivated: event<() => void>().api,
    },
    tabGroups: { query: async () => [], update: async () => undefined },
    sidePanel: { setPanelBehavior: async () => undefined },
  };
  const background = new Background({
    chrome,
    hostName: 'com.aura.chrome_control.ttest',
    timers: { set: () => 0, clear: () => undefined },
    now: () => 0,
    sleep: async () => undefined,
  });
  background.start();
  return { background, native, connects };
}

const settled = () => new Promise((done) => setTimeout(done, 0));

async function connected() {
  const h = harness();
  h.native.receive({ type: 'authenticated' } satisfies CoreMessage);
  await settled();
  return h;
}

describe('the background', () => {
  it('answers a command with its result, and nothing for a release or a withdrawn command', async () => {
    const h = await connected();
    h.native.receive({
      type: 'command',
      id: 3,
      command: { op: 'console', tab_id: 7, level: null, max_entries: 5 },
    });
    h.native.receive({ type: 'command', id: 0, command: { op: 'release', tab_ids: [7] } });
    h.native.receive({ type: 'command', id: 4, command: { op: 'evaluate', tab_id: 7 } });
    await settled();
    // In the order they finish, which is not the order they came in.
    const results = h.native.sent
      .filter((message) => (message as { type?: string }).type === 'result')
      .sort((one, two) => (one as { id: number }).id - (two as { id: number }).id);
    expect(results).toEqual([
      {
        type: 'result',
        id: 3,
        outcome: {
          status: 'done',
          value: { url: TAB.url, entries: [], truncated: false },
        },
      },
      {
        type: 'result',
        id: 4,
        outcome: {
          status: 'failed',
          code: 'chrome.unsupported',
          message: 'Aura 发来的命令这个扩展版本不认识；请更新扩展。',
        },
      },
    ]);
  });

  it('drops the answer to a command Core withdrew', async () => {
    const h = await connected();
    h.native.receive({
      type: 'command',
      id: 5,
      command: { op: 'assist', tab_id: 7, reason: '请登录', timeout_seconds: 60 },
    });
    await settled();
    expect(h.background.state.assist?.reason).toBe('请登录');
    h.native.receive({ type: 'cancel', id: 5 });
    await settled();
    expect(h.background.state.assist).toBeNull();
    expect(h.native.sent.some((message) => (message as { id?: number }).id === 5)).toBe(false);
  });

  it('ends a handover with the user’s 完成 in Aura', async () => {
    const h = await connected();
    h.native.receive({
      type: 'command',
      id: 6,
      command: { op: 'assist', tab_id: 7, reason: '请登录', timeout_seconds: 60 },
    });
    await settled();
    h.native.receive({ type: 'assist_done', tab_id: 8 });
    await settled();
    expect(h.background.state.assist?.tab_id).toBe(7);
    h.native.receive({ type: 'assist_done', tab_id: 7 });
    await vi.waitFor(() =>
      expect(h.native.sent).toContainEqual(
        expect.objectContaining({
          id: 6,
          outcome: expect.objectContaining({ value: expect.objectContaining({ outcome: 'done' }) }),
        }),
      ),
    );
  });

  it('shows a side panel the state, and passes on what the user asks', async () => {
    const h = await connected();
    const panel = port(PANEL_PORT);
    h.connects.fire(panel.value);
    expect((panel.sent[0] as PanelState).connection).toEqual({ kind: 'connected' });
    h.native.receive({
      type: 'sessions',
      sessions: [{ session_id: 'ses_one', title: 'Docs', workspace: 'Aura' }],
    });
    await settled();
    expect((panel.sent.at(-1) as PanelState).sessions).toHaveLength(1);
    panel.receive({ type: 'stop' });
    expect(h.native.sent.at(-1)).toEqual({ type: 'stop' });
  });
});
