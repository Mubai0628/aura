// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { ipcRenderer } from 'electron';
import { browserBridge, readPageState } from '../preload/browser.js';

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
    message: '浏览器没有回应',
  });
  invoke.mockResolvedValueOnce([state, { bad: true }]);
  await expect(bridge.pages()).resolves.toEqual([state]);
});
