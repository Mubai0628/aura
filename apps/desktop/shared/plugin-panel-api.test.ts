// @vitest-environment node
import { expect, test } from 'vitest';
import {
  PANEL_CAPABILITIES_ARGUMENT,
  panelCallValue,
  readPanelCapabilities,
  readPanelResult,
  readPanelState,
} from './plugin-panel-api.js';

const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';

test("the window takes Main's panel answers only in their shape", () => {
  expect(readPanelResult({ ok: true })).toEqual({ ok: true, picture: undefined });
  expect(readPanelResult({ ok: true, picture: 'data:image/jpeg;base64,AAAA' })).toEqual({
    ok: true,
    picture: 'data:image/jpeg;base64,AAAA',
  });
  // Only a JPEG Main encoded is drawn in the panel's place.
  expect(readPanelResult({ ok: true, picture: 'javascript:alert(1)' })).toEqual({
    ok: true,
    picture: undefined,
  });
  expect(readPanelResult({ ok: false, message: '插件面板已关闭' })).toEqual({
    ok: false,
    message: '插件面板已关闭',
  });
  expect(readPanelResult(undefined)).toEqual({ ok: false, message: '插件面板没有回应' });
  expect(
    readPanelState({ sessionId: S, plugin: 'usage-panel', stopped: '面板停止运行了' }),
  ).toEqual({ sessionId: S, plugin: 'usage-panel', stopped: '面板停止运行了' });
  expect(readPanelState({ sessionId: S, plugin: 'usage-panel' })).toBeUndefined();
});

test("a panel page's preload offers only what Main says its plugin declared", () => {
  expect([
    ...readPanelCapabilities([
      'electron',
      `${PANEL_CAPABILITIES_ARGUMENT}session.read,session.send`,
    ]),
  ]).toEqual(['session.read', 'session.send']);
  expect(readPanelCapabilities([`${PANEL_CAPABILITIES_ARGUMENT}`]).size).toBe(0);
  expect(readPanelCapabilities(['--other=session.control']).size).toBe(0);
});

test('a page reads the value of a call, or the refusal with its code as an error', () => {
  expect(panelCallValue({ ok: true, value: { running: false } })).toEqual({ running: false });
  expect(() =>
    panelCallValue({
      ok: false,
      message: 'plugin_session.capability_missing: 没有声明 session.send',
    }),
  ).toThrow('plugin_session.capability_missing: 没有声明 session.send');
  expect(() => panelCallValue('nothing')).toThrow('plugin_panel.failed');
});
