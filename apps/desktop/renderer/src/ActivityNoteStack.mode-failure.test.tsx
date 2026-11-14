import { createRef } from 'react';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { AgentModeEpisodeView } from '@aura/contracts/generated';
import { ActivityNoteStack } from './ActivityNoteStack';

// A Plan or Goal control Core refused says which control, why, and what to do: a budget below
// what the Goal has used, a mode that changed first, or no answer from Core.

const rect = (width: number): DOMRect => ({
  bottom: 100,
  height: 100,
  left: 0,
  right: width,
  toJSON: () => ({}),
  top: 0,
  width,
  x: 0,
  y: 0,
});

beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      disconnect() {}
      observe() {}
      unobserve() {}
    },
  );
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement,
  ) {
    if (this.classList.contains('activity-stack')) return rect(240);
    if (this.classList.contains('activity-test-conversation')) return rect(400);
    if (this.classList.contains('activity-test-viewport')) return rect(1_000);
    return rect(0);
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const goal = {
  session_id: 'session',
  revision: '3',
  mode: 'goal',
  paused: false,
  goal_budget: { max_tokens: 2000, max_turns: 8 },
  inherited_goal: {
    budget: { max_tokens: 2000, max_turns: 8 },
    completed: false,
    consumed_tokens: 400,
    consumed_turns: 2,
    revision: '3',
    turn_task_id: 'turn',
  },
} as unknown as AgentModeEpisodeView;

const refused = (message: string, code = 'agent.request_rejected') =>
  Object.assign(new Error(message), { code });

const show = (onControl: (action: unknown) => Promise<void>) => {
  const conversationColumnRef = createRef<HTMLDivElement>();
  const viewportRef = createRef<HTMLDivElement>();
  render(
    <div className="activity-test-viewport" ref={viewportRef}>
      <div className="activity-test-conversation" ref={conversationColumnRef} />
      <ActivityNoteStack
        combinedSidebarWidthRatio={0.2}
        conversationColumnRef={conversationColumnRef}
        viewportRef={viewportRef}
        episode={goal}
        onControl={onControl}
      />
    </div>,
  );
};

test('a budget below what the Goal has used says so and what to fill in', async () => {
  show(
    vi.fn().mockRejectedValue(refused('persistent state violates agent.goal_budget_below_usage')),
  );
  fireEvent.click(screen.getByRole('button', { name: '编辑额度' }));
  fireEvent.change(screen.getByLabelText('模型步数上限'), { target: { value: '1' } });
  fireEvent.click(screen.getByRole('button', { name: '保存额度' }));
  expect(await screen.findByRole('alert')).toHaveProperty(
    'textContent',
    '新的额度低于这个目标已经用掉的量，没有保存；请填更大的数，或留空表示不设上限。',
  );
});

test('a mode that changed first names the control that did not take effect', async () => {
  show(vi.fn().mockRejectedValue(refused('persistent state violates task_lineage.revision_stale')));
  fireEvent.click(screen.getByRole('button', { name: '暂停' }));
  expect(await screen.findByRole('alert')).toHaveProperty(
    'textContent',
    '模式状态刚刚变化，这次暂停没有生效；请按最新状态再操作。',
  );
});

test('a control Core did not answer says why and that the mode is unchanged', async () => {
  show(vi.fn().mockRejectedValue(refused('Core request timed out', 'ipc.request_timeout')));
  fireEvent.click(screen.getByRole('button', { name: '终止' }));
  expect(await screen.findByRole('alert')).toHaveProperty(
    'textContent',
    '这次终止没有生效（Aura Core 没有及时回复），模式保持原样；请稍后再试。',
  );
});
