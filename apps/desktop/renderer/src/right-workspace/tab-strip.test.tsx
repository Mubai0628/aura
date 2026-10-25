import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi, type Mock } from 'vitest';
import { ToolTabStrip, type StripTab } from './ToolTabStrip';
import type { ToolTab } from './tool-tabs';

// Tabs give
// way, the wheel scrolls them sideways, the tab shown stays in view and 全部标签 lists them all;
// a tab's right-click menu closes it, the others or those to its right.

const tab = (id: string, kind: ToolTab['kind'], label = id, givesWay = false): StripTab => ({
  tab: { id, kind } as ToolTab,
  label,
  icon: kind === 'terminal' ? 'terminal' : 'review',
  givesWay,
});
// A model's terminal is named after its command; a 文件 page keeps its name.
const tabs = [
  tab('终端 1', 'terminal'),
  tab('审查', 'review'),
  tab('m', 'terminal', 'cargo test --workspace', true),
];

let scrolled: Mock<(options?: boolean | ScrollIntoViewOptions) => void>;
beforeEach(() => {
  scrolled = vi.fn();
  Element.prototype.scrollIntoView = scrolled;
});
const original = Element.prototype.scrollIntoView;
afterEach(() => {
  cleanup();
  Element.prototype.scrollIntoView = original;
});

function strip(overrides: Partial<Parameters<typeof ToolTabStrip>[0]> = {}) {
  const props = {
    tabs,
    active: '审查',
    onActivate: vi.fn(),
    onClose: vi.fn(),
    onCloseMany: vi.fn(),
    ...overrides,
  };
  render(<ToolTabStrip {...props} />);
  return props;
}

/** jsdom lays nothing out: the row is made wider inside than it is shown. */
function overflowing(width: number, inside: number) {
  const row = screen.getByRole('tablist', { name: '已打开的工具' });
  Object.defineProperty(row, 'clientWidth', { configurable: true, value: width });
  Object.defineProperty(row, 'scrollWidth', { configurable: true, value: inside });
  fireEvent.scroll(row);
  return row;
}

test('the wheel scrolls a row too wide for the sidebar sideways', () => {
  strip();
  const row = overflowing(200, 600);
  row.scrollLeft = 0;
  const wheel = new WheelEvent('wheel', { deltaY: 120, bubbles: true, cancelable: true });
  row.dispatchEvent(wheel);
  expect(row.scrollLeft).toBe(120);
  expect(wheel.defaultPrevented).toBe(true);
});

test('全部标签 comes only while the tabs cannot fit the whole row, and goes once they can', () => {
  strip();
  const row = screen.getByRole('tablist', { name: '已打开的工具' });
  Object.defineProperty(row.parentElement, 'clientWidth', { configurable: true, value: 240 });
  overflowing(240, 300);
  expect(screen.getByRole('button', { name: '全部标签' })).toBeTruthy();
  // The button takes its room from the row: tabs that fit without it must not keep it there.
  overflowing(210, 236);
  expect(screen.queryByRole('button', { name: '全部标签' })).toBeNull();
});

test('a model’s terminal, named after its command, gives way first; a tool keeps its name', () => {
  strip();
  const ways = screen
    .getAllByRole('tab')
    .map((item) => item.closest('.tool-tab')?.classList.contains('gives-way'));
  expect(ways).toEqual([false, false, true]);
});

test('the tab shown is brought into view', () => {
  const props = strip();
  expect(scrolled.mock.contexts.at(-1)).toBe(
    screen.getByRole('tab', { name: /审查/u }).closest('.tool-tab'),
  );
  cleanup();
  strip({ ...props, active: 'm' });
  expect(scrolled.mock.contexts.at(-1)).toBe(
    screen.getByRole('tab', { name: /cargo test/u }).closest('.tool-tab'),
  );
});

test('with tabs out of view, 全部标签 lists them all and picks one', async () => {
  const user = userEvent.setup();
  const props = strip();
  expect(screen.queryByRole('button', { name: '全部标签' })).toBeNull();
  overflowing(200, 600);
  await user.click(await screen.findByRole('button', { name: '全部标签' }));
  const list = screen.getByRole('menu', { name: '全部标签' });
  expect(
    within(list)
      .getAllByRole('menuitem')
      .map((item) => item.textContent),
  ).toEqual(['终端 1', '审查', 'cargo test --workspace']);
  await user.click(within(list).getByRole('menuitem', { name: '终端 1' }));
  expect(props.onActivate).toHaveBeenCalledWith(tabs[0]!.tab);
});

// A 文件 page's files have their own small tabs and menus; a sidebar tab's menu only closes.
test('a tab’s menu closes it, the others or those to its right, and holds nothing else', async () => {
  const user = userEvent.setup();
  const props = strip();
  const menuOf = (name: RegExp) => {
    fireEvent.contextMenu(screen.getByRole('tab', { name }));
    return screen.getByRole('menu');
  };
  await user.click(within(menuOf(/审查/u)).getByRole('menuitem', { name: '关闭其他标签' }));
  expect(props.onCloseMany).toHaveBeenLastCalledWith([tabs[0]!.tab, tabs[2]!.tab]);
  await user.click(within(menuOf(/终端 1/u)).getByRole('menuitem', { name: '关闭右侧标签' }));
  expect(props.onCloseMany).toHaveBeenLastCalledWith([tabs[1]!.tab, tabs[2]!.tab]);
  const last = menuOf(/cargo test/u);
  expect(
    within(last).getByRole('menuitem', { name: '关闭右侧标签' }).getAttribute('aria-disabled'),
  ).toBe('true');
  expect(within(last).queryByRole('menuitem', { name: '复制路径' })).toBeNull();
  await user.click(within(last).getByRole('menuitem', { name: '关闭' }));
  expect(props.onClose).toHaveBeenCalledWith(tabs[2]!.tab);
});
