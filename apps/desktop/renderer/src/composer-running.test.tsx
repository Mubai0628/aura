import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { SessionModelSelectorProjection } from '@aura/contracts/generated';
import { ResizableComposer } from './ResizableComposer';
import { resetComposerDraftsForTests } from './agent-session/composer-drafts';

// While a task runs the composer has no row of lane buttons. With nothing
// typed the send button stops the task; with a message it queues it for the next turn, where the
// queue offers to append it at once. A Plan or Goal chip carries its own way back to normal.

const selector = {
  binding_revision: '1',
  selected_model_profile_id: 'model',
  models: [],
  endpoints: [],
  providers: [],
} as unknown as SessionModelSelectorProjection;
const submit = vi.fn();
const start = vi.fn();
const stop = vi.fn();
// The task modes are built-in commands of Core's catalog.
const builtin = (name: string, title: string) => ({
  name,
  title,
  description: title,
  kind: 'immediate',
  source: 'builtin',
  plugin: null,
  argument_hint: null,
  template: null,
  enabled: true,
  issue: null,
  location: null,
  revision: null,
});
beforeEach(() => {
  localStorage.clear();
  resetComposerDraftsForTests();
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  start.mockReset().mockResolvedValue({ ok: true, value: {} });
  stop.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listCommands: vi.fn().mockResolvedValue({
          ok: true,
          value: { commands: [builtin('plan', 'Plan 模式'), builtin('goal', 'Goal 模式')] },
        }),
        listSkills: vi.fn().mockResolvedValue({
          ok: true,
          value: { revision: 1, skills: [], roots: [], issue: null },
        }),
        submitAgentMessage: submit,
        startAgent: start,
      },
    },
  });
});
afterEach(cleanup);

function renderComposer(running: boolean) {
  render(
    <ResizableComposer
      sessionId="session"
      activeTurn={running ? 'turn' : undefined}
      activeRunId={running ? 'run' : undefined}
      workspaceId="workspace"
      queuePending={false}
      onChanged={vi.fn()}
      onStop={stop}
      demoMode={false}
      modelSelector={selector}
      onOpenModelSettings={vi.fn()}
      onOpenPromptInspector={vi.fn()}
      onOpenSecuritySettings={vi.fn()}
      onOpenSettings={vi.fn()}
      onOpenPanel={vi.fn()}
      onSelectModel={vi.fn()}
      onSetPermissionMode={vi.fn()}
      permissionMode="read_only"
      promptInspector={undefined}
    />,
  );
  return screen.getByRole('textbox', { name: '输入消息' }) as HTMLTextAreaElement;
}

test('with a task running and nothing typed, the send button stops the task', async () => {
  const user = userEvent.setup();
  renderComposer(true);
  const composer = screen.getByRole('region', { name: '消息编辑器' });
  expect(composer.textContent).not.toContain('发送将进入下一轮');
  expect(within(composer).queryByRole('button', { name: '追加到当前任务' })).toBeNull();
  await user.click(within(composer).getByRole('button', { name: '停止当前任务' }));
  expect(stop).toHaveBeenCalledTimes(1);
  expect(submit).not.toHaveBeenCalled();
});

test('typing turns the stop button back into send, and the message joins the queue', async () => {
  const user = userEvent.setup();
  const box = renderComposer(true);
  await user.type(box, '再看看测试');
  const composer = screen.getByRole('region', { name: '消息编辑器' });
  expect(within(composer).queryByRole('button', { name: '停止当前任务' })).toBeNull();
  await user.click(within(composer).getByRole('button', { name: '加入下一轮队列' }));
  expect(submit).toHaveBeenCalledWith({
    session_id: 'session',
    text: '再看看测试',
    destination: { lane: 'next_turn' },
  });
  expect(stop).not.toHaveBeenCalled();
});

// /plan and /goal choose the mode; the + menu holds no modes; the chip's × leaves as before.
test('/plan chooses Plan; its chip has its own × that goes back to a normal conversation', async () => {
  const user = userEvent.setup();
  const box = renderComposer(false);
  await user.type(box, '/plan');
  await user.keyboard('{Enter}');
  expect(screen.getByText('Plan', { selector: '.composer-mode-chip *' })).toBeTruthy();
  expect((box as HTMLTextAreaElement).value).toBe('');
  await user.click(screen.getByRole('button', { name: '退出 Plan 模式' }));
  expect(screen.queryByText('Plan', { selector: '.composer-mode-chip *' })).toBeNull();
  await user.click(screen.getByRole('button', { name: '添加' }));
  expect(screen.queryByRole('menuitemradio')).toBeNull();
});

test('/goal chooses Goal, and its budgets open from its chip', async () => {
  const user = userEvent.setup();
  const box = renderComposer(false);
  await user.type(box, '/goal');
  await user.keyboard('{Enter}');
  await user.click(screen.getByRole('button', { name: 'Goal 预算' }));
  await user.type(screen.getByLabelText('Goal Token 预算'), '2000000');
  await user.type(screen.getByLabelText('Goal 模型步数预算'), '50');
  await user.type(box, '做一个小游戏');
  await user.click(screen.getByRole('button', { name: '发送消息' }));
  await vi.waitFor(() =>
    expect(start).toHaveBeenCalledWith(
      expect.objectContaining({
        text: '做一个小游戏',
        mode: 'goal',
        goal_budget: { max_tokens: 2000000, max_turns: 50 },
      }),
    ),
  );
});
