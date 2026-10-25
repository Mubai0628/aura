import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { useContextMenu, type MenuEntry } from './useContextMenu';

// right-click menus for links, tabs and files. One menu at the pointer:
// arrows move, Enter picks, Escape or a click elsewhere closes it, and an item that cannot be
// used says why.

afterEach(cleanup);

function Harness({ entries }: { entries: MenuEntry[] }) {
  const menu = useContextMenu();
  return (
    <>
      <button onContextMenu={(event) => menu.open(event, '文件操作', entries)} type="button">
        目标
      </button>
      <button type="button">别处</button>
      {menu.menu}
    </>
  );
}

const entries = () => {
  const first = vi.fn();
  const second = vi.fn();
  const blocked = vi.fn();
  return {
    first,
    second,
    blocked,
    list: [
      { label: '第一项', run: first },
      'separator',
      { label: '第二项', run: second },
      { label: '不能用的项', disabled: true, note: '这里说明原因', run: blocked },
    ] as MenuEntry[],
  };
};

test('the menu opens at the pointer; arrows move, Enter picks and it closes', async () => {
  const user = userEvent.setup();
  const { first, second, list } = entries();
  render(<Harness entries={list} />);
  fireEvent.contextMenu(screen.getByRole('button', { name: '目标' }), {
    clientX: 120,
    clientY: 80,
  });
  const menu = screen.getByRole('menu', { name: '文件操作' });
  expect(menu.style.left).toBe('120px');
  expect(menu.style.top).toBe('80px');
  expect(document.activeElement?.textContent).toBe('第一项');
  await user.keyboard('{ArrowDown}');
  expect(document.activeElement?.textContent).toBe('第二项');
  await user.keyboard('{Enter}');
  expect(second).toHaveBeenCalledOnce();
  expect(first).not.toHaveBeenCalled();
  expect(screen.queryByRole('menu')).toBeNull();
});

test('an item that cannot be used says why and does nothing', async () => {
  const user = userEvent.setup();
  const { blocked, list } = entries();
  render(<Harness entries={list} />);
  fireEvent.contextMenu(screen.getByRole('button', { name: '目标' }));
  const item = screen.getByRole('menuitem', { name: /不能用的项/u });
  expect(item.getAttribute('aria-disabled')).toBe('true');
  expect(item.textContent).toContain('这里说明原因');
  await user.click(item);
  expect(blocked).not.toHaveBeenCalled();
  expect(screen.getByRole('menu')).toBeTruthy();
});

test('Escape or a click elsewhere closes the menu', async () => {
  const user = userEvent.setup();
  render(<Harness entries={entries().list} />);
  const target = screen.getByRole('button', { name: '目标' });
  fireEvent.contextMenu(target);
  await user.keyboard('{Escape}');
  expect(screen.queryByRole('menu')).toBeNull();
  fireEvent.contextMenu(target);
  await user.click(screen.getByRole('button', { name: '别处' }));
  expect(screen.queryByRole('menu')).toBeNull();
});

test('scrolling the app does by itself leaves the menu open; the wheel elsewhere closes it', () => {
  render(<Harness entries={entries().list} />);
  fireEvent.contextMenu(screen.getByRole('button', { name: '目标' }));
  const menu = screen.getByRole('menu', { name: '文件操作' });
  // The conversation following an answer, or a tab brought into view as it is right-clicked.
  fireEvent.scroll(screen.getByRole('button', { name: '别处' }));
  fireEvent.scroll(document);
  expect(screen.getByRole('menu')).toBe(menu);
  fireEvent.wheel(menu);
  expect(screen.getByRole('menu')).toBe(menu);
  fireEvent.wheel(screen.getByRole('button', { name: '别处' }));
  expect(screen.queryByRole('menu')).toBeNull();
});
