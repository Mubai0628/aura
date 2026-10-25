import type { GoalState } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test } from 'vitest';
import { GoalDock } from './GoalDock';
import { forgetGoalMemory } from './goal-memory';

//

function goal(completed: boolean, turn = 'trn_1', revision = '1'): GoalState {
  return {
    budget: { max_tokens: null, max_turns: 40 },
    completed,
    consumed_tokens: 6_182_228,
    consumed_turns: 9,
    latest_check_model_step: null,
    revision,
    turn_task_id: turn,
  };
}

beforeEach(() => {
  localStorage.clear();
  forgetGoalMemory();
});
afterEach(cleanup);

/** A window that starts again: what was remembered comes from localStorage alone. */
function reopen() {
  cleanup();
  forgetGoalMemory();
}

test('a Goal under way says so, moves, and shows what it has used live', () => {
  const { container } = render(
    <GoalDock goal={goal(false)} objective="帮我写一个饥饿鲨鱼HTML小游戏" paused={false} running />,
  );
  expect(screen.getByRole('heading', { name: 'Goal' })).toBeTruthy();
  const card = container.querySelector('.goal-dock');
  expect(card?.getAttribute('data-state')).toBe('running');
  expect(screen.getByText('进行中')).toBeTruthy();
  expect(screen.getByText('帮我写一个饥饿鲨鱼HTML小游戏')).toBeTruthy();
  expect(container.querySelector('.goal-progress')).toBeTruthy();
  expect(container.textContent).toContain('Token 6,182,228 / 未设上限');
  expect(container.textContent).toContain('模型步数 9 / 40');
  // Nothing to close while the Goal is under way.
  expect(screen.queryByRole('button', { name: '关闭 Goal' })).toBeNull();
});

test('a turn that ended with the Goal open shows it not finished, and a paused one as paused', () => {
  const { container, rerender } = render(
    <GoalDock goal={goal(false)} paused={false} running={false} />,
  );
  expect(container.querySelector('.goal-dock')?.getAttribute('data-state')).toBe('open');
  expect(screen.getByText('未完成')).toBeTruthy();
  expect(container.querySelector('.goal-progress')).toBeNull();
  rerender(<GoalDock goal={goal(false)} paused running={false} />);
  expect(screen.getByText('已暂停')).toBeTruthy();
  expect(screen.queryByRole('button', { name: '关闭 Goal' })).toBeNull();
});

test('a completed Goal can be closed, stays closed, and a new Goal shows again', () => {
  render(<GoalDock goal={goal(true)} paused={false} running={false} />);
  expect(screen.getByText('已完成')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '关闭 Goal' }));
  expect(screen.queryByRole('heading', { name: 'Goal' })).toBeNull();

  reopen();
  render(<GoalDock goal={goal(true)} paused={false} running={false} />);
  expect(screen.queryByRole('heading', { name: 'Goal' })).toBeNull();

  reopen();
  render(<GoalDock goal={goal(false, 'trn_2')} paused={false} running />);
  expect(screen.getByRole('heading', { name: 'Goal' })).toBeTruthy();
});

test('without a Goal there is no card', () => {
  const { container } = render(<GoalDock goal={undefined} paused={false} running={false} />);
  expect(container.textContent).toBe('');
});
