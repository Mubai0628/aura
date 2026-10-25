import type { TodoState, TodoStatus } from '@aura/contracts/generated';
import { useId, useReducer } from 'react';
import { AppIcon } from '../app-icons';
import { closeTodo, foldTodo, isTodoFolded, todoListKey } from './todo-memory';
import { finished, todoProgress } from './todo-progress';
import './todo-dock.css';

/** What a screen reader hears for each item; the marks say the same at a glance. */
const STATUS_WORDS: Record<TodoStatus, string> = {
  pending: '待处理',
  in_progress: '进行中',
  completed: '已完成',
  blocked: '受阻',
  cancelled: '已取消',
};

/**
 * The model's task list for the conversation's latest turn. The heading counts
 * what is done and a bar shows it; the item being worked on turns. Folded, it keeps one line: that
 * item. Once every item is done it can be closed, and that list stays closed; a list the model
 * writes afterwards shows again. A part of the 近期活动 card, under its own small title like the card's others.
 */
export function TodoDock({ sessionId, todo }: { sessionId: string; todo: TodoState | undefined }) {
  const headingId = useId();
  const [, remembered] = useReducer((count: number) => count + 1, 0);
  const progress = todoProgress(todo);
  if (!todo || !progress) return null;
  const list = todoListKey(todo.turn_task_id, todo.revision);
  const { done, total } = progress;
  const complete = done === total;
  const current =
    todo.items.find((item) => item.status === 'in_progress') ??
    todo.items.find((item) => !finished(item));
  const folded = isTodoFolded(sessionId);

  return (
    <section
      aria-labelledby={headingId}
      className="activity-section todo-dock"
      data-complete={complete || undefined}
      data-folded={folded || undefined}
    >
      <div className="todo-head">
        <h4 className="activity-section-title" id={headingId}>
          待办
        </h4>
        {folded && current ? (
          <span className={`todo-item todo-head-current is-${current.status}`}>
            <TodoMark status={current.status} />
            <span className="todo-label" title={current.title}>
              {current.title}
            </span>
          </span>
        ) : null}
        <span aria-label={`已完成 ${done} / ${total}`} className="todo-count">
          {done}/{total}
        </span>
        <button
          aria-expanded={!folded}
          aria-label={folded ? '展开清单' : '收起清单'}
          className="todo-icon-button todo-fold"
          onClick={() => {
            foldTodo(sessionId, !folded);
            remembered();
          }}
          type="button"
        >
          <AppIcon name="chevron-down" size={14} />
        </button>
        {complete ? (
          <button
            aria-label="关闭清单"
            className="todo-icon-button"
            onClick={() => {
              closeTodo(list);
              remembered();
            }}
            type="button"
          >
            <AppIcon name="close" size={13} />
          </button>
        ) : null}
      </div>
      <span aria-hidden="true" className="todo-progress">
        <span style={{ width: `${(done / total) * 100}%` }} />
      </span>
      {folded ? null : (
        <ul>
          {todo.items.map((item) => (
            <li className={`todo-item is-${item.status}`} key={item.id}>
              <TodoMark status={item.status} />
              <span className="todo-label" title={item.title}>
                {item.title}
              </span>
              <span className="todo-status-words">{STATUS_WORDS[item.status]}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function TodoMark({ status }: { status: TodoStatus }) {
  return (
    <span aria-hidden="true" className={`todo-mark is-${status}`}>
      {status === 'completed' ? <AppIcon name="check" size={10} /> : null}
      {status === 'blocked' ? <AppIcon name="warning" size={10} /> : null}
      {status === 'cancelled' ? <AppIcon name="minus" size={10} /> : null}
    </span>
  );
}
