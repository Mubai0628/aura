import type { TodoState, TodoStatus } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test } from 'vitest';
import { TodoDock } from './TodoDock';
import { forgetTodoMemory } from './todo-memory';

// "优化ToDo卡片美术设计、增加折叠、进行中ToDo动态效果，且模型全部完成后，ToDoList的
// 前端卡片可由用户手动关闭"; the "只读" note goes (no explanatory text).

function list(statuses: TodoStatus[], revision = '1', turn = 'tsk_1'): TodoState {
  return {
    items: statuses.map((status, index) => ({
      id: `tdi_${index}`,
      status,
      title: `第 ${index + 1} 项`,
    })),
    revision,
    turn_task_id: turn,
  };
}

beforeEach(() => {
  localStorage.clear();
  forgetTodoMemory();
});
afterEach(cleanup);

/** A window that starts again: what was remembered comes from localStorage alone. */
function reopen() {
  cleanup();
  forgetTodoMemory();
}

test('the card counts what is done and marks the item being worked on, without a read-only note', () => {
  const { container } = render(
    <TodoDock sessionId="ses_a" todo={list(['completed', 'in_progress', 'pending', 'blocked'])} />,
  );
  expect(screen.getByRole('heading', { name: '待办' })).toBeTruthy();
  expect(screen.getByLabelText('已完成 1 / 4').textContent).toBe('1/4');
  expect(container.textContent).not.toContain('只读');
  const working = screen.getByText('第 2 项').closest('li');
  expect(working?.className).toContain('is-in_progress');
  expect(working?.textContent).toContain('进行中');
  expect(screen.getByText('第 4 项').closest('li')?.className).toContain('is-blocked');
  const bar = container.querySelector<HTMLElement>('.todo-progress > span');
  expect(bar?.style.width).toBe('25%');
  // Nothing to close while work remains.
  expect(screen.queryByRole('button', { name: '关闭清单' })).toBeNull();
});

test('folded, the card keeps the item being worked on, and the conversation remembers it', () => {
  render(<TodoDock sessionId="ses_a" todo={list(['completed', 'in_progress', 'pending'])} />);
  fireEvent.click(screen.getByRole('button', { name: '收起清单' }));
  expect(screen.queryByRole('list')).toBeNull();
  expect(screen.getByText('第 2 项')).toBeTruthy();
  expect(screen.getByRole('button', { name: '展开清单' }).getAttribute('aria-expanded')).toBe(
    'false',
  );

  reopen();
  render(<TodoDock sessionId="ses_a" todo={list(['completed', 'in_progress', 'pending'])} />);
  expect(screen.queryByRole('list')).toBeNull();
  reopen();
  render(<TodoDock sessionId="ses_b" todo={list(['completed', 'in_progress', 'pending'])} />);
  expect(screen.getByRole('list')).toBeTruthy();
});

test('a finished list closes and stays closed; the next list the model writes shows', () => {
  render(<TodoDock sessionId="ses_a" todo={list(['completed', 'cancelled', 'completed'])} />);
  expect(screen.getByLabelText('已完成 3 / 3')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '关闭清单' }));
  expect(screen.queryByRole('heading', { name: '待办' })).toBeNull();

  reopen();
  render(<TodoDock sessionId="ses_a" todo={list(['completed', 'cancelled', 'completed'])} />);
  expect(screen.queryByRole('heading', { name: '待办' })).toBeNull();
  reopen();
  render(<TodoDock sessionId="ses_a" todo={list(['completed', 'pending'], '2')} />);
  expect(screen.getByRole('heading', { name: '待办' })).toBeTruthy();
});

test('no items, no card', () => {
  const { container } = render(<TodoDock sessionId="ses_a" todo={list([])} />);
  expect(container.innerHTML).toBe('');
});
