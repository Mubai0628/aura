// @vitest-environment node
import { expect, test } from 'vitest';
import {
  PANEL_DEDUPE_KEY_MAX_CHARS,
  PANEL_MESSAGE_MAX_CHARS,
  PANEL_NOTICE_MAX_CHARS,
  PANEL_SETTING_MAX_CHARS,
  PANEL_STATUS_MAX_CHARS,
} from '../../shared/plugin-panel-api.js';
import { CallBudget, coreCall, declared, readPanelCall, shownItem } from './calls.js';

const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';

test('a panel call is taken only in exactly its shape', () => {
  expect(readPanelCall({ op: 'state' })).toEqual({ op: 'state' });
  expect(readPanelCall({ op: 'stop' })).toEqual({ op: 'stop' });
  expect(readPanelCall({ op: 'events' })).toEqual({ op: 'events' });
  expect(readPanelCall({ op: 'events', after: 12 })).toEqual({ op: 'events', after: 12 });
  expect(readPanelCall({ op: 'send', text: '继续' })).toEqual({ op: 'send', text: '继续' });
  expect(readPanelCall({ op: 'send', text: '继续', dedupe_key: 'qq:1:42' })).toEqual({
    op: 'send',
    text: '继续',
    dedupe_key: 'qq:1:42',
  });
  for (const value of [
    null,
    'state',
    [],
    { op: 'delete' },
    // The page never names the plugin or the conversation.
    { op: 'state', session_id: S },
    { op: 'stop', plugin: 'other' },
    { op: 'events', after: -1 },
    { op: 'events', after: 1.5 },
    { op: 'events', after: '3' },
    { op: 'send' },
    { op: 'send', text: '   ' },
    { op: 'send', text: 'x'.repeat(PANEL_MESSAGE_MAX_CHARS + 1) },
    { op: 'send', text: 'hi', dedupe_key: '' },
    { op: 'send', text: 'hi', dedupe_key: 'a\nb' },
    { op: 'send', text: 'hi', dedupe_key: 'k'.repeat(PANEL_DEDUPE_KEY_MAX_CHARS + 1) },
    { op: 'send', text: 'hi', session_id: S },
  ])
    expect(readPanelCall(value), JSON.stringify(value)).toBeUndefined();
  // A message as long as Core takes is taken, counted in characters.
  expect(readPanelCall({ op: 'send', text: '字'.repeat(PANEL_MESSAGE_MAX_CHARS) })).toBeDefined();
});

test("every call acts in the panel's own conversation, for its own plugin", () => {
  expect(coreCall({ op: 'state' }, 'usage-panel', S)).toEqual({
    method: 'agent.plugin_session.state',
    params: { plugin: 'usage-panel', session_id: S },
  });
  expect(coreCall({ op: 'events', after: 7 }, 'usage-panel', S).params).toEqual({
    plugin: 'usage-panel',
    session_id: S,
    after: 7,
  });
  expect(coreCall({ op: 'send', text: 'hi', dedupe_key: 'k' }, 'usage-panel', S)).toEqual({
    method: 'agent.plugin_session.send',
    params: { plugin: 'usage-panel', session_id: S, text: 'hi', dedupe_key: 'k' },
  });
  expect(coreCall({ op: 'stop' }, 'usage-panel', S).method).toBe('agent.plugin_session.stop');
});

test('a call needs the capability its plugin declared', () => {
  const capabilities = ['session.read', 'session.events'];
  expect(declared({ op: 'state' }, capabilities)).toBe(true);
  expect(declared({ op: 'events' }, capabilities)).toBe(true);
  expect(declared({ op: 'send', text: 'hi' }, capabilities)).toBe(false);
  expect(declared({ op: 'stop' }, capabilities)).toBe(false);
});

test('a page that calls in a loop is held back, and its messages sooner', () => {
  let now = 0;
  const budget = new CallBudget(4, 2, 1_000, () => now);
  expect(budget.take('send')).toBe(true);
  expect(budget.take('send')).toBe(true);
  expect(budget.take('send')).toBe(false);
  expect(budget.take('state')).toBe(true);
  expect(budget.take('events')).toBe(true);
  expect(budget.take('state')).toBe(false);
  // The window moves on.
  now = 1_000;
  expect(budget.take('send')).toBe(true);
  expect(budget.take('state')).toBe(true);
});

test('a notice and the status entry are taken only in exactly their shape', () => {
  expect(readPanelCall({ op: 'notify', text: '新消息' })).toEqual({ op: 'notify', text: '新消息' });
  expect(readPanelCall({ op: 'notify', text: '断开', tone: 'error' })).toEqual({
    op: 'notify',
    text: '断开',
    tone: 'error',
  });
  expect(readPanelCall({ op: 'status', text: '在线', tone: 'success' })).toEqual({
    op: 'status',
    text: '在线',
    tone: 'success',
  });
  // No text, or a blank one, clears the entry.
  expect(readPanelCall({ op: 'status' })).toEqual({ op: 'status' });
  expect(readPanelCall({ op: 'status', text: '  ', tone: 'info' })).toEqual({
    op: 'status',
    tone: 'info',
  });
  // As long as Core takes, counted in characters.
  expect(readPanelCall({ op: 'notify', text: '字'.repeat(PANEL_NOTICE_MAX_CHARS) })).toBeDefined();
  expect(readPanelCall({ op: 'status', text: '字'.repeat(PANEL_STATUS_MAX_CHARS) })).toBeDefined();
  for (const value of [
    { op: 'notify' },
    { op: 'notify', text: '   ' },
    { op: 'notify', text: 'x'.repeat(PANEL_NOTICE_MAX_CHARS + 1) },
    { op: 'notify', text: 'hi', tone: 'loud' },
    { op: 'notify', text: 'hi', session_id: S },
    { op: 'status', text: 'x'.repeat(PANEL_STATUS_MAX_CHARS + 1) },
    { op: 'status', text: 3 },
    { op: 'status', text: null },
    { op: 'status', tone: 'danger' },
    { op: 'status', text: 'hi', plugin: 'other' },
  ])
    expect(readPanelCall(value), JSON.stringify(value)).toBeUndefined();
});

test("a notice and the status entry are the plugin's own and name no conversation", () => {
  expect(coreCall({ op: 'notify', text: 'hi', tone: 'warning' }, 'qq-bridge', S)).toEqual({
    method: 'agent.plugin_session.notify',
    params: { plugin: 'qq-bridge', text: 'hi', tone: 'warning' },
  });
  expect(coreCall({ op: 'status', text: '在线' }, 'qq-bridge', S)).toEqual({
    method: 'agent.plugin_session.status',
    params: { plugin: 'qq-bridge', text: '在线' },
  });
  expect(coreCall({ op: 'status' }, 'qq-bridge', S).params).toEqual({ plugin: 'qq-bridge' });
  expect(declared({ op: 'notify', text: 'hi' }, ['notify'])).toBe(true);
  expect(declared({ op: 'status' }, ['notify'])).toBe(false);
  expect(declared({ op: 'status', text: 'x' }, ['status'])).toBe(true);
});

test('Main shows what Core checked, and only for the plugin that asked', () => {
  expect(shownItem({ plugin: 'qq-bridge', text: '在线', tone: 'success' }, 'qq-bridge')).toEqual({
    text: '在线',
    tone: 'success',
  });
  // Core's answer without text clears the entry.
  expect(shownItem({ plugin: 'qq-bridge', tone: 'info' }, 'qq-bridge')).toEqual({
    text: undefined,
    tone: 'info',
  });
  for (const value of [
    undefined,
    'ok',
    { plugin: 'other-plugin', text: 'x', tone: 'info' },
    { plugin: 'qq-bridge', text: 3, tone: 'info' },
    { plugin: 'qq-bridge', text: 'x', tone: 'loud' },
    { plugin: 'qq-bridge', text: 'x' },
  ])
    expect(shownItem(value, 'qq-bridge'), JSON.stringify(value)).toBeUndefined();
});

test("the plugin's own command is run in the panel's conversation; its settings name none", () => {
  expect(readPanelCall({ op: 'run', name: 'summary' })).toEqual({ op: 'run', name: 'summary' });
  const run = { op: 'run', name: 'summary', arguments: 'src/lib.rs', dedupe_key: 'k-1' };
  expect(readPanelCall(run)).toEqual(run);
  expect(readPanelCall({ op: 'settings' })).toEqual({ op: 'settings' });
  const set = { op: 'set_setting', key: 'greeting', value: '你好' };
  expect(readPanelCall(set)).toEqual(set);
  // An empty value clears the setting; one as long as Core takes is taken.
  expect(readPanelCall({ ...set, value: '' })).toEqual({ ...set, value: '' });
  expect(readPanelCall({ ...set, value: '字'.repeat(PANEL_SETTING_MAX_CHARS) })).toBeDefined();
  for (const value of [
    { op: 'run' },
    { op: 'run', name: '' },
    { op: 'run', name: '/summary' },
    { op: 'run', name: 'Summary' },
    { op: 'run', name: 'summary', arguments: 3 },
    { op: 'run', name: 'summary', arguments: 'x'.repeat(PANEL_MESSAGE_MAX_CHARS + 1) },
    { op: 'run', name: 'summary', dedupe_key: 'a\nb' },
    { op: 'run', name: 'summary', session_id: S },
    { op: 'settings', plugin: 'other' },
    { op: 'set_setting', key: 'greeting' },
    { op: 'set_setting', key: 'Greeting', value: 'x' },
    { op: 'set_setting', key: 'greeting', value: 3 },
    { op: 'set_setting', key: 'greeting', value: 'x'.repeat(PANEL_SETTING_MAX_CHARS + 1) },
  ])
    expect(readPanelCall(value), JSON.stringify(value)).toBeUndefined();

  expect(
    coreCall({ op: 'run', name: 'summary', arguments: 'src', dedupe_key: 'k' }, 'own', S),
  ).toEqual({
    method: 'agent.plugin_session.run',
    params: { plugin: 'own', session_id: S, name: 'summary', arguments: 'src', dedupe_key: 'k' },
  });
  expect(coreCall({ op: 'run', name: 'summary' }, 'own', S).params).toEqual({
    plugin: 'own',
    session_id: S,
    name: 'summary',
  });
  expect(coreCall({ op: 'settings' }, 'own', S)).toEqual({
    method: 'agent.plugin_session.settings',
    params: { plugin: 'own' },
  });
  expect(coreCall({ op: 'set_setting', key: 'greeting', value: '' }, 'own', S)).toEqual({
    method: 'agent.plugin_session.set_setting',
    params: { plugin: 'own', key: 'greeting', value: '' },
  });
  expect(declared({ op: 'run', name: 'summary' }, ['commands'])).toBe(true);
  expect(declared({ op: 'run', name: 'summary' }, ['session.send'])).toBe(false);
  expect(declared({ op: 'settings' }, ['settings'])).toBe(true);
  expect(declared({ op: 'set_setting', key: 'k', value: 'v' }, ['settings'])).toBe(true);
});

test('running a command and setting a setting are held back as messages are', () => {
  const budget = new CallBudget(10, 2, 1_000, () => 0);
  expect(budget.take('run')).toBe(true);
  expect(budget.take('set_setting')).toBe(true);
  expect(budget.take('send')).toBe(false);
  expect(budget.take('settings')).toBe(true);
});
