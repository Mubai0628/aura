import { act, cleanup, render, renderHook, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import type { PluginStatusEntry, PluginUiChange } from '../../../shared/plugin-ui-api';
import type { NotificationInput } from '../notification-center-store';
import { PluginStatusLine } from './PluginStatusLine';
import { usePluginNotices, usePluginStatuses } from './plugin-ui';

// Plugins show one status entry each, in their row on the 插件 settings page, and their notices
// in the notification center under their name. The window shows only what Main sends.

let listeners: ((change: PluginUiChange) => void)[];
let answer: (statuses: PluginStatusEntry[]) => void;

beforeEach(() => {
  listeners = [];
  vi.stubGlobal('aura', {
    v1: {
      pluginUi: {
        statuses: () =>
          new Promise<PluginStatusEntry[]>((resolve) => {
            answer = resolve;
          }),
        onChange: (listener: (change: PluginUiChange) => void) => {
          listeners.push(listener);
          return () => {
            listeners = listeners.filter((each) => each !== listener);
          };
        },
      },
    },
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const change = (value: PluginUiChange) =>
  act(() => {
    for (const listener of listeners) listener(value);
  });

test('the entries are what Main sends, and follow its changes', async () => {
  const { result } = renderHook(() => usePluginStatuses());
  expect(result.current).toEqual([]);

  await act(async () => answer([{ plugin: 'qq-bridge', text: '已连接', tone: 'success' }]));
  expect(result.current).toEqual([{ plugin: 'qq-bridge', text: '已连接', tone: 'success' }]);

  const statuses: PluginStatusEntry[] = [
    { plugin: 'qq-bridge', text: '重连中', tone: 'warning' },
    { plugin: 'usage-panel', text: '12k', tone: 'info' },
  ];
  change({ kind: 'statuses', statuses });
  expect(result.current).toEqual(statuses);

  // A notice is not a status entry.
  change({ kind: 'notice', notice: { plugin: 'qq-bridge', text: '新消息', tone: 'info' } });
  expect(result.current).toEqual(statuses);

  change({ kind: 'statuses', statuses: [] });
  expect(result.current).toEqual([]);
});

test('a change that came first is newer than the entries the window asked for', async () => {
  const { result } = renderHook(() => usePluginStatuses());
  change({ kind: 'statuses', statuses: [{ plugin: 'qq-bridge', text: '断开', tone: 'error' }] });
  await act(async () => answer([{ plugin: 'qq-bridge', text: '已连接', tone: 'success' }]));
  expect(result.current).toEqual([{ plugin: 'qq-bridge', text: '断开', tone: 'error' }]);
});

test("an entry shows its text in its tone's colour, and whose it is on hover", () => {
  render(<PluginStatusLine entry={{ plugin: 'qq-bridge', text: '重连中', tone: 'warning' }} />);
  const line = screen.getByText('重连中').closest('p');
  expect(line?.textContent).toBe('重连中');
  expect(line?.className).toBe('plugin-status-line state-warning');
  expect(line?.title).toBe('插件 qq-bridge：重连中');
  cleanup();
  render(<PluginStatusLine entry={{ plugin: 'usage-panel', text: '12k', tone: 'info' }} />);
  expect(screen.getByText('12k').closest('p')?.className).toBe('plugin-status-line state-muted');
});

test("a plugin's notice goes to the notification center under its name", () => {
  const published: NotificationInput[] = [];
  const { unmount } = renderHook(() =>
    usePluginNotices((input: NotificationInput) => published.push(input)),
  );
  change({
    kind: 'notice',
    notice: { plugin: 'qq-bridge', text: '断开了，正在重连', tone: 'error' },
  });
  change({ kind: 'notice', notice: { plugin: 'qq-bridge', text: '又连上了', tone: 'success' } });
  change({ kind: 'statuses', statuses: [] });
  expect(published).toEqual([
    {
      dedupeKey: 'plugin:qq-bridge:断开了，正在重连',
      title: '插件 qq-bridge',
      message: '断开了，正在重连',
      tone: 'danger',
    },
    {
      dedupeKey: 'plugin:qq-bridge:又连上了',
      title: '插件 qq-bridge',
      message: '又连上了',
      tone: 'success',
    },
  ]);
  unmount();
  expect(listeners).toEqual([]);
});

test('a window without the bridge shows no entries and takes no notices', () => {
  vi.stubGlobal('aura', { v1: {} });
  const { result } = renderHook(() => usePluginStatuses());
  renderHook(() => usePluginNotices(() => undefined));
  expect(result.current).toEqual([]);
  expect(listeners).toEqual([]);
});
