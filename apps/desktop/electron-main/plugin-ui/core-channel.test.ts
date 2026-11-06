// @vitest-environment node
import { expect, test } from 'vitest';
import type { PluginUiChange } from '../../shared/plugin-ui-api.js';
import { connectServiceUi, pluginUiNotification, readServiceUi } from './core-channel.js';
import { PluginUiHub, type HubClock } from './hub.js';

// Each reading is two seconds on, so the status bar's one-second quiet never holds a change.
let time = 0;
const clock: HubClock = { now: () => (time += 2_000), later: () => () => undefined };

function connected() {
  const changes: PluginUiChange[] = [];
  const hub = new PluginUiHub((change) => changes.push(change), clock);
  connectServiceUi(hub);
  return { hub, changes };
}

const notice = { plugin: 'chat-bot', text: '已连接', tone: 'success' };
const status = { plugin: 'chat-bot', text: '在线', tone: 'info' };

test("a service's changes reach the hub as the service's own", () => {
  const { hub, changes } = connected();
  expect(pluginUiNotification('plugin.ui.v1', { kind: 'notice', notice }, 'run-1')).toBe(true);
  expect(changes).toContainEqual({ kind: 'notice', notice });
  pluginUiNotification('plugin.ui.v1', { kind: 'status', status }, 'run-1');
  expect(hub.statuses()).toEqual([status]);
  // A panel's entry for the same plugin is another source; the service's going leaves it.
  hub.status('chat-bot', 'panel:1', '面板', 'info');
  pluginUiNotification('plugin.ui.v1', { kind: 'ended', plugin: 'chat-bot' }, 'run-1');
  expect(hub.statuses()).toEqual([{ plugin: 'chat-bot', text: '面板', tone: 'info' }]);
  pluginUiNotification('plugin.ui.v1', { kind: 'service', plugin: 'chat-bot' }, 'run-1');
  expect(changes.at(-1)).toEqual({ kind: 'service', plugin: 'chat-bot' });
});

test('a status without text clears the service entry', () => {
  const { hub } = connected();
  pluginUiNotification('plugin.ui.v1', { kind: 'status', status }, 'run-1');
  const cleared = { plugin: 'chat-bot', tone: 'info' };
  pluginUiNotification('plugin.ui.v1', { kind: 'status', status: cleared }, 'run-1');
  expect(hub.statuses()).toEqual([]);
});

test("a new Core starts with none of the old one's service entries", () => {
  const { hub } = connected();
  pluginUiNotification('plugin.ui.v1', { kind: 'status', status }, 'run-1');
  hub.status('usage-panel', 'panel:1', '面板', 'info');
  const other = { plugin: 'mail-bot', text: '同步中', tone: 'info' };
  pluginUiNotification('plugin.ui.v1', { kind: 'status', status: other }, 'run-2');
  expect(hub.statuses()).toEqual([{ plugin: 'usage-panel', text: '面板', tone: 'info' }, other]);
});

test('only plugin.ui.v1 is taken, and only in its exact shape', () => {
  const { changes } = connected();
  expect(pluginUiNotification('browser.command.v1', { kind: 'notice', notice }, 'run-1')).toBe(
    false,
  );
  for (const value of [
    null,
    'notice',
    { kind: 'notice' },
    { kind: 'notice', notice: { ...notice, plugin: 'QQ' } },
    { kind: 'notice', notice: { ...notice, text: '' } },
    { kind: 'notice', notice: { ...notice, tone: 'loud' } },
    { kind: 'notice', notice, extra: true },
    { kind: 'status', status: { ...status, text: 3 } },
    { kind: 'ended', plugin: '../x' },
    { kind: 'service' },
    { kind: 'restart', plugin: 'chat-bot' },
  ]) {
    expect(readServiceUi(value)).toBeUndefined();
    expect(pluginUiNotification('plugin.ui.v1', value, 'run-1')).toBe(true);
  }
  expect(changes).toEqual([]);
});
