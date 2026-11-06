import { act, cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import type {
  PluginPanelCommand,
  PluginPanelResult,
  PluginPanelState,
} from '../../../shared/plugin-panel-api';
import { PluginPanel } from './PluginPanel';

// A plugin's panel is a page Main lays over its tab's area. It opens each time the tab is shown
// (Core says each time whether the plugin can still be used); one that stopped says why, in its
// place, and opens again when asked.

const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
let command: ReturnType<typeof vi.fn<(value: PluginPanelCommand) => Promise<PluginPanelResult>>>;
let stateListener: ((state: PluginPanelState) => void) | undefined;

beforeEach(() => {
  command = vi.fn(async () => ({ ok: true }));
  stateListener = undefined;
  vi.stubGlobal('aura', {
    v1: {
      pluginPanels: {
        command,
        onState: (listener: (state: PluginPanelState) => void) => {
          stateListener = listener;
          return () => {
            stateListener = undefined;
          };
        },
      },
    },
  });
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
    return this.classList.contains('plugin-panel-area')
      ? new DOMRect(1200, 60, 400, 800)
      : new DOMRect(0, 0, 0, 0);
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const kinds = () => command.mock.calls.map(([value]) => value.kind);
const settle = () => act(async () => new Promise((done) => setTimeout(done, 40)));

test('a shown tab opens its panel and lays it over the tab; a hidden one does neither', async () => {
  const { rerender } = render(<PluginPanel plugin="usage-panel" sessionId={S} visible={false} />);
  await settle();
  expect(command).not.toHaveBeenCalled();

  rerender(<PluginPanel plugin="usage-panel" sessionId={S} visible />);
  await settle();
  expect(command.mock.calls[0]![0]).toEqual({ kind: 'open', sessionId: S, plugin: 'usage-panel' });
  expect(command.mock.calls.find(([value]) => value.kind === 'show')?.[0]).toEqual({
    kind: 'show',
    sessionId: S,
    plugin: 'usage-panel',
    bounds: { x: 1200, y: 60, width: 400, height: 800 },
  });

  rerender(<PluginPanel plugin="usage-panel" sessionId={S} visible={false} />);
  await settle();
  expect(kinds().at(-1)).toBe('hide');
});

test('a panel Core no longer offers says why in its place and is not laid over the tab', async () => {
  command.mockImplementation(async (value) =>
    value.kind === 'open'
      ? { ok: false, message: '插件 usage-panel 的面板现在不能用：没有 panel/index.html' }
      : { ok: true },
  );
  render(<PluginPanel plugin="usage-panel" sessionId={S} visible />);
  expect((await screen.findByRole('status')).textContent).toBe(
    '插件 usage-panel 的面板现在不能用：没有 panel/index.html',
  );
  expect(kinds()).not.toContain('show');
});

test('a panel that stopped says why, and opens again when asked', async () => {
  const user = userEvent.setup();
  render(<PluginPanel plugin="usage-panel" sessionId={S} visible />);
  await settle();
  expect(kinds()).toContain('show');

  // Another conversation's panel stopping is not this one's.
  act(() =>
    stateListener?.({
      sessionId: 'ses_01BX5ZZKBKACTAV9WEVGEMMVRZ',
      plugin: 'usage-panel',
      stopped: 'x',
    }),
  );
  expect(screen.queryByRole('status')).toBeNull();

  act(() =>
    stateListener?.({ sessionId: S, plugin: 'usage-panel', stopped: '面板停止运行了（crashed）' }),
  );
  expect((await screen.findByRole('status')).textContent).toBe('面板停止运行了（crashed）');
  command.mockClear();
  await user.click(screen.getByRole('button', { name: '重新打开' }));
  await settle();
  expect(kinds()[0]).toBe('open');
  expect(kinds()).toContain('show');
  expect(screen.queryByRole('status')).toBeNull();
});

test('while a menu covers the panel its picture stands in its place', async () => {
  command.mockImplementation(async (value) =>
    value.kind === 'picture' ? { ok: true, picture: 'data:image/jpeg;base64,AAAA' } : { ok: true },
  );
  const { container } = render(<PluginPanel plugin="usage-panel" sessionId={S} visible />);
  await settle();
  const menu = document.createElement('div');
  menu.setAttribute('role', 'menu');
  menu.getBoundingClientRect = () => new DOMRect(1300, 100, 160, 120);
  await act(async () => {
    document.body.append(menu);
    await new Promise((done) => setTimeout(done, 250));
  });
  expect(kinds()).toContain('picture');
  expect(kinds().at(-1)).toBe('hide');
  expect(container.querySelector('img.plugin-panel-picture')?.getAttribute('src')).toBe(
    'data:image/jpeg;base64,AAAA',
  );
  menu.remove();
});
