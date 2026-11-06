// @vitest-environment node
import { expect, test } from 'vitest';
import { readPanelCommand, readPanelSite, shownMessage } from './commands.js';

const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const bounds = { x: 10, y: 20, width: 300, height: 400 };

test("the window's panel commands are taken only in exactly their shape", () => {
  for (const kind of ['open', 'hide', 'close', 'picture'] as const)
    expect(readPanelCommand({ kind, sessionId: S, plugin: 'usage-panel' })).toEqual({
      kind,
      sessionId: S,
      plugin: 'usage-panel',
    });
  expect(readPanelCommand({ kind: 'show', sessionId: S, plugin: 'usage-panel', bounds })).toEqual({
    kind: 'show',
    sessionId: S,
    plugin: 'usage-panel',
    bounds,
  });
  for (const value of [
    { kind: 'reload', sessionId: S, plugin: 'usage-panel' },
    { kind: 'open', sessionId: 'ses_x', plugin: 'usage-panel' },
    { kind: 'open', sessionId: S, plugin: 'Usage' },
    { kind: 'open', sessionId: S, plugin: '../x' },
    { kind: 'open', sessionId: S, plugin: 'usage-panel', folder: 'C:\\' },
    { kind: 'show', sessionId: S, plugin: 'usage-panel' },
    { kind: 'show', sessionId: S, plugin: 'usage-panel', bounds: { ...bounds, width: 0 } },
    { kind: 'show', sessionId: S, plugin: 'usage-panel', bounds: { ...bounds, x: -1 } },
    { kind: 'show', sessionId: S, plugin: 'usage-panel', bounds: { ...bounds, y: Number.NaN } },
    { kind: 'show', sessionId: S, plugin: 'usage-panel', bounds: { ...bounds, z: 1 } },
    null,
  ])
    expect(readPanelCommand(value), JSON.stringify(value)).toBeUndefined();
});

test("Core's description of a panel is taken in exactly its shape", () => {
  const answer = {
    title: '本会话用量',
    folder: 'C:\\Users\\me\\plugins\\usage-panel',
    entry: 'panel/index.html',
    network: ['https://api.example.com'],
    capabilities: ['session.read'],
  };
  expect(readPanelSite(answer, 'usage-panel')).toEqual({ plugin: 'usage-panel', ...answer });
  for (const wrong of [
    { ...answer, entry: 'index.html' },
    { ...answer, folder: '' },
    { ...answer, network: ['http://api.example.com'] },
    { ...answer, network: ['https://api.example.com/v1'] },
    { ...answer, network: ['https://*.example.com'] },
    { ...answer, capabilities: 'session.read' },
    { ...answer, extra: true },
  ])
    expect(readPanelSite(wrong, 'usage-panel'), JSON.stringify(wrong)).toBeUndefined();
});

test('the user reads a refusal without its code', () => {
  expect(
    shownMessage(
      { envelope: { message: 'plugin_session.panel_unusable: 插件 usage-panel 的面板现在不能用' } },
      'x',
    ),
  ).toBe('插件 usage-panel 的面板现在不能用');
  expect(shownMessage(new Error('plugin_panel.core_unavailable: Aura Core 还没有就绪'), 'x')).toBe(
    'Aura Core 还没有就绪',
  );
  expect(shownMessage(undefined, '插件面板没有打开')).toBe('插件面板没有打开');
});
