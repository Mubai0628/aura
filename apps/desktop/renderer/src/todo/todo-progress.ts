import type { TodoItem, TodoState } from '@aura/contracts/generated';
import { isTodoClosed, todoListKey } from './todo-memory';

/** An item nobody is still working towards. */
export const finished = (item: TodoItem) =>
  item.status === 'completed' || item.status === 'cancelled';

/**
 * How far a task list has got, or nothing once it is empty or its user closed it. The collapsed 近期活动 card shows it too.
 */
export function todoProgress(
  todo: TodoState | undefined,
): { done: number; total: number } | undefined {
  if (!todo?.items.length) return undefined;
  if (isTodoClosed(todoListKey(todo.turn_task_id, todo.revision))) return undefined;
  return { done: todo.items.filter(finished).length, total: todo.items.length };
}
