// @vitest-environment node
import { expect, test } from 'vitest';
import {
  PLUGIN_STATUS_ENTRIES_MAX,
  readPluginStatuses,
  readPluginUiChange,
} from './plugin-ui-api.js';

const entry = { plugin: 'qq-bridge', text: '已连接', tone: 'success' };

test("Main's status entries reach the window only as what they are", () => {
  expect(readPluginStatuses([entry])).toEqual([entry]);
  // An entry that is not one is left out; the others stay.
  expect(
    readPluginStatuses([
      entry,
      { ...entry, plugin: 'QQ' },
      { ...entry, plugin: '../x' },
      { ...entry, text: '' },
      { ...entry, text: 3 },
      { ...entry, tone: 'loud' },
      null,
      [],
    ]),
  ).toEqual([entry]);
  // Only the fields the window shows are kept, and a text no longer than Core allows a notice.
  expect(readPluginStatuses([{ ...entry, extra: true }])).toEqual([entry]);
  expect(readPluginStatuses([{ ...entry, text: '字'.repeat(300) }])[0]?.text).toHaveLength(200);
  expect(readPluginStatuses(Array.from({ length: 40 }, () => entry))).toHaveLength(
    PLUGIN_STATUS_ENTRIES_MAX,
  );
  expect(readPluginStatuses('x')).toEqual([]);
  expect(readPluginStatuses(undefined)).toEqual([]);
});

test("Main's changes reach the window only as what they are", () => {
  expect(readPluginUiChange({ kind: 'statuses', statuses: [entry] })).toEqual({
    kind: 'statuses',
    statuses: [entry],
  });
  expect(readPluginUiChange({ kind: 'statuses', statuses: [] })).toEqual({
    kind: 'statuses',
    statuses: [],
  });
  const notice = { plugin: 'qq-bridge', text: '断开了', tone: 'error' };
  expect(readPluginUiChange({ kind: 'notice', notice })).toEqual({ kind: 'notice', notice });
  // A plugin's background service changed state: the plugin page reads its plugins again.
  expect(readPluginUiChange({ kind: 'service', plugin: 'qq-bridge' })).toEqual({
    kind: 'service',
    plugin: 'qq-bridge',
  });
  for (const value of [
    null,
    'notice',
    { kind: 'notice' },
    { kind: 'notice', notice: { ...notice, tone: 'danger' } },
    { kind: 'notice', notice: { ...notice, text: '' } },
    { kind: 'statuses', statuses: 'x' },
    { kind: 'statuses' },
    { kind: 'other', statuses: [] },
    { kind: 'service' },
    { kind: 'service', plugin: 'QQ' },
  ])
    expect(readPluginUiChange(value), JSON.stringify(value)).toBeUndefined();
});
