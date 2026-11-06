// @vitest-environment node
import { expect, test } from 'vitest';
import type { PluginUiChange } from '../../shared/plugin-ui-api.js';
import {
  NOTICE_WINDOW_MS,
  NOTICES_PER_WINDOW,
  PluginUiHub,
  STATUS_QUIET_MS,
  type HubClock,
} from './hub.js';

/** A clock the test moves by hand; what waits runs once its time has come. */
function handClock() {
  let now = 0;
  let waiting: { at: number; run: () => void; cancelled: boolean }[] = [];
  const clock: HubClock = {
    now: () => now,
    later: (run, ms) => {
      const entry = { at: now + ms, run, cancelled: false };
      waiting.push(entry);
      return () => {
        entry.cancelled = true;
      };
    },
  };
  const advance = (ms: number) => {
    const until = now + ms;
    for (;;) {
      const due = waiting
        .filter((entry) => !entry.cancelled && entry.at <= until)
        .sort((a, b) => a.at - b.at)[0];
      if (due === undefined) break;
      waiting = waiting.filter((entry) => entry !== due);
      now = due.at;
      due.run();
    }
    now = until;
  };
  return { clock, advance, pending: () => waiting.filter((entry) => !entry.cancelled).length };
}

function hub() {
  const changes: PluginUiChange[] = [];
  const time = handClock();
  return { hub: new PluginUiHub((change) => changes.push(change), time.clock), changes, ...time };
}

const shown = (changes: PluginUiChange[]) =>
  changes.flatMap((change) => (change.kind === 'statuses' ? [change.statuses] : []));

test('a plugin shows at most three notices in ten seconds; another plugin has its own', () => {
  const { hub: ui, changes, advance } = hub();
  for (let index = 0; index < NOTICES_PER_WINDOW; index += 1) {
    expect(ui.noticeAllowed('qq-bridge')).toBe(true);
    expect(ui.notice({ plugin: 'qq-bridge', text: `第 ${index} 条`, tone: 'info' })).toBe(true);
    advance(1_000);
  }
  expect(ui.noticeAllowed('qq-bridge')).toBe(false);
  expect(ui.notice({ plugin: 'qq-bridge', text: '太多了', tone: 'warning' })).toBe(false);
  expect(ui.notice({ plugin: 'usage-panel', text: '别的插件', tone: 'success' })).toBe(true);
  expect(changes.filter((change) => change.kind === 'notice')).toHaveLength(NOTICES_PER_WINDOW + 1);
  expect(changes.at(-1)).toEqual({
    kind: 'notice',
    notice: { plugin: 'usage-panel', text: '别的插件', tone: 'success' },
  });

  // Ten seconds after the first one, there is room for one more.
  advance(NOTICE_WINDOW_MS - NOTICES_PER_WINDOW * 1_000);
  expect(ui.notice({ plugin: 'qq-bridge', text: '又可以了', tone: 'info' })).toBe(true);
  expect(ui.notice({ plugin: 'qq-bridge', text: '还不行', tone: 'info' })).toBe(false);
});

test('a status entry changes on screen at most once a second, and the last value stays', () => {
  const { hub: ui, changes, advance, pending } = hub();
  ui.status('qq-bridge', 'panel:a', '连接中', 'info');
  expect(shown(changes)).toEqual([[{ plugin: 'qq-bridge', text: '连接中', tone: 'info' }]]);

  // Changes inside the second wait; only the last of them is shown, once.
  ui.status('qq-bridge', 'panel:a', '重连 1', 'warning');
  ui.status('qq-bridge', 'panel:a', '重连 2', 'warning');
  ui.status('qq-bridge', 'panel:a', '已连接', 'success');
  expect(shown(changes)).toHaveLength(1);
  expect(pending()).toBe(1);
  advance(STATUS_QUIET_MS);
  expect(shown(changes)).toEqual([
    [{ plugin: 'qq-bridge', text: '连接中', tone: 'info' }],
    [{ plugin: 'qq-bridge', text: '已连接', tone: 'success' }],
  ]);
  expect(ui.statuses()).toEqual([{ plugin: 'qq-bridge', text: '已连接', tone: 'success' }]);

  // The same value again changes nothing.
  advance(STATUS_QUIET_MS);
  ui.status('qq-bridge', 'panel:a', '已连接', 'success');
  expect(shown(changes)).toHaveLength(2);

  // A change back to what is shown before its time comes is no change at all.
  ui.status('qq-bridge', 'panel:a', '断开', 'error');
  expect(shown(changes)).toHaveLength(3);
  ui.status('qq-bridge', 'panel:a', '重连', 'warning');
  ui.status('qq-bridge', 'panel:a', '断开', 'error');
  advance(STATUS_QUIET_MS);
  expect(shown(changes)).toHaveLength(3);

  // Clearing it takes the entry away, at the same pace.
  advance(STATUS_QUIET_MS);
  ui.status('qq-bridge', 'panel:a', undefined, 'info');
  expect(ui.statuses()).toEqual([]);
  expect(shown(changes).at(-1)).toEqual([]);
});

test("an entry belongs to what set it; the plugin's newest one shows, and goes with its source", () => {
  const { hub: ui, changes, advance } = hub();
  ui.status('qq-bridge', 'panel:a', '会话 A', 'info');
  advance(STATUS_QUIET_MS);
  ui.status('qq-bridge', 'panel:b', '会话 B', 'info');
  advance(STATUS_QUIET_MS);
  ui.status('usage-panel', 'panel:a', '12k', 'info');
  expect(ui.statuses()).toEqual([
    { plugin: 'qq-bridge', text: '会话 B', tone: 'info' },
    { plugin: 'usage-panel', text: '12k', tone: 'info' },
  ]);

  // The panel that set the newest one closes: the other panel's entry shows again.
  advance(STATUS_QUIET_MS);
  ui.drop('qq-bridge', 'panel:b');
  expect(ui.statuses()[0]).toEqual({ plugin: 'qq-bridge', text: '会话 A', tone: 'info' });

  // Something that held nothing changes nothing.
  const before = changes.length;
  ui.drop('qq-bridge', 'service');
  ui.drop('other-plugin', 'panel:a');
  expect(changes).toHaveLength(before);

  // The last one goes: the plugin has no entry.
  advance(STATUS_QUIET_MS);
  ui.drop('qq-bridge', 'panel:a');
  expect(ui.statuses()).toEqual([{ plugin: 'usage-panel', text: '12k', tone: 'info' }]);
});
