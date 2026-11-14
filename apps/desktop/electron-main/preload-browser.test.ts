// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { ipcRenderer } from 'electron';
import { browserBridge, readPageState } from '../preload/browser.js';
import { PAGE_TEXT_CHARS } from '../shared/browser-api.js';

vi.mock('electron', () => ({
  ipcRenderer: { on: vi.fn(), removeListener: vi.fn(), invoke: vi.fn() },
}));

const state = {
  sessionId: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  pageId: 'tab-1',
  url: 'https://example.com/',
  title: 'Example',
  loading: false,
  canGoBack: true,
  canGoForward: false,
  agentActive: false,
  userInControl: false,
  closed: false,
  notice: { kind: 'blocked', text: 'example.com 已在浏览器设置中被阻止' },
};

test('a page state reaches the renderer only in its exact shape', () => {
  expect(readPageState(state)).toEqual(state);
  expect(readPageState({ ...state, notice: undefined })).toEqual({ ...state, notice: undefined });
  for (const bad of [
    undefined,
    { ...state, url: 7 },
    { ...state, loading: 'no' },
    { ...state, notice: { kind: 'script', text: 'x' } },
    { ...state, notice: 'blocked' },
  ])
    expect(readPageState(bad)).toBeUndefined();
});

// The model's request for the user's help reaches the renderer with the page's state.
test("a model's request for help passes in its exact shape", () => {
  const asking = { ...state, pageId: 'model', assist: { reason: '请登录', deadline: 1_700_000 } };
  expect(readPageState(asking)).toEqual(asking);
  for (const bad of [
    { ...asking, assist: { reason: 7, deadline: 1 } },
    { ...asking, assist: { reason: 'x', deadline: 'soon' } },
    { ...asking, assist: 'please' },
  ])
    expect(readPageState(bad)).toBeUndefined();
});

// A page the user handed to the model with "@" keeps the id the model's tools take for it.
test('a page the user handed over reaches the renderer with its id, in its exact shape', () => {
  const handed = { ...state, handedAs: 'p1' };
  expect(readPageState(handed)).toEqual(handed);
  for (const bad of [
    { ...state, handedAs: 7 },
    { ...state, handedAs: 'model' },
    { ...state, handedAs: 'p0' },
    { ...state, handedAs: 'p12345' },
  ])
    expect(readPageState(bad)).toBeUndefined();
});

// What "@" sends of a page is its text as Main read it, and a page handed over comes back named.
test("a page's text and a handed page's state pass the bridge in their exact shape", async () => {
  const invoke = vi.mocked(ipcRenderer.invoke);
  const bridge = browserBridge(vi.fn());
  const text = {
    title: 'Example',
    url: 'https://example.com/',
    text: '计数：0\n加一',
    truncated: false,
  };
  const read = () => bridge.command({ kind: 'text', sessionId: state.sessionId, pageId: 'tab-1' });
  invoke.mockResolvedValueOnce({ ok: true, text });
  await expect(read()).resolves.toEqual({ ok: true, text });
  invoke.mockResolvedValueOnce({ ok: true, state: { ...state, handedAs: 'p1' } });
  await expect(
    bridge.command({ kind: 'hand-over', sessionId: state.sessionId, pageId: 'tab-1' }),
  ).resolves.toEqual({ ok: true, state: { ...state, handedAs: 'p1' } });
  for (const bad of [
    { ...text, text: 7 },
    { ...text, truncated: 'no' },
    { ...text, title: undefined },
    { ...text, text: 'x'.repeat(PAGE_TEXT_CHARS + 1) },
    'text',
  ]) {
    invoke.mockResolvedValueOnce({ ok: true, text: bad });
    await expect(read()).resolves.toEqual({ ok: true });
  }
});

test('the bridge sends commands on its own channel and drops an invalid reply', async () => {
  const invoke = vi.mocked(ipcRenderer.invoke);
  const bridge = browserBridge(vi.fn());
  invoke.mockResolvedValueOnce({ ok: true, state });
  await expect(
    bridge.command({ kind: 'open', sessionId: state.sessionId, pageId: 'tab-1' }),
  ).resolves.toEqual({
    ok: true,
    state,
  });
  expect(invoke).toHaveBeenLastCalledWith('aura:v1:browser-command', {
    kind: 'open',
    sessionId: state.sessionId,
    pageId: 'tab-1',
  });
  invoke.mockResolvedValueOnce({ surprise: true });
  await expect(
    bridge.command({ kind: 'reload', sessionId: state.sessionId, pageId: 'tab-1' }),
  ).resolves.toEqual({
    ok: false,
    message:
      '这次浏览器操作的结果读不懂（Aura 内部出错），操作可能没有完成；请重试，仍不行请重启 Aura。',
  });
  invoke.mockRejectedValueOnce(new Error("No handler registered for 'aura:v1:browser-command'"));
  await expect(
    bridge.command({ kind: 'open', sessionId: state.sessionId, pageId: 'tab-1' }),
  ).resolves.toEqual({
    ok: false,
    message: '浏览器没有回应，页面没有打开；请稍后重试，仍不行时重启 Aura。',
  });
  invoke.mockRejectedValueOnce(new Error('gone'));
  await expect(bridge.command({ kind: 'clear-data' })).resolves.toEqual({
    ok: false,
    message: '浏览器没有回应，浏览数据没有清除；请稍后重试，仍不行时重启 Aura。',
  });
  invoke.mockResolvedValueOnce([state, { bad: true }]);
  await expect(bridge.pages()).resolves.toEqual([state]);
});
