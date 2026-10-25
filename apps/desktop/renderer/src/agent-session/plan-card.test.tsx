import type { PlanRecord } from '@aura/contracts/generated';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { PlanCard } from './PlanCard';

// The plan the model
// wrote is Markdown; it was shown as plain text, `**` and backticks and all, every step in bold.

afterEach(cleanup);

const plan = {
  turn_task_id: 'turn',
  revision: '1',
  at: '2026-10-20T00:00:00Z',
  original_user_goal: '写一个走迷宫的2D游戏HTML',
  plan: {
    expected_turn_revision: '3',
    current_project_state:
      '`C:\\work\\demo` 是 `demo-site`。仓库根目录同时存放若干**独立单文件 HTML 演示页**。',
    requirements: ['产出单文件 `maze-game.html`', '迷宫由程序化算法生成，且**起点到终点必定连通**'],
    steps: [
      {
        id: 'S1',
        description: '**实现迷宫内核**：用递归回溯（DFS）生成完美迷宫。',
        validation: '对 200 个随机尺寸迷宫断言 `BFS` 可达。',
      },
      {
        id: 'S2',
        description: '渲染层：Canvas 2D 程序化绘制。',
        validation: '在桩 ctx 下调用 60 帧无异常。',
        depends_on: ['S1'],
      },
    ],
  },
} as unknown as PlanRecord;

test('the plan reads as written: Markdown rendered, nothing left as raw marks', () => {
  render(<PlanCard plan={plan} disabled={false} onImplement={vi.fn()} />);
  const card = screen.getByRole('region', { name: '实施计划 · 版本 1' });
  expect(card.textContent).not.toMatch(/\*\*|`/u);
  expect(within(card).getByText('C:\\work\\demo').tagName).toBe('CODE');
  // Streamdown marks bold text on the element it renders.
  expect(within(card).getByText('独立单文件 HTML 演示页').dataset.streamdown).toBe('strong');
  expect(within(card).getByText('起点到终点必定连通').dataset.streamdown).toBe('strong');
  expect(within(card).getByText('写一个走迷宫的2D游戏HTML')).toBeTruthy();
});

test('each step shows its id, its description in normal weight, how it is checked and what it waits for', () => {
  render(<PlanCard plan={plan} disabled={false} onImplement={vi.fn()} />);
  const steps = within(screen.getByRole('list', { name: '步骤' })).getAllByRole('listitem');
  expect(steps).toHaveLength(2);
  expect(within(steps[0]!).getByText('S1').className).toContain('agent-plan-step-id');
  // Only what the plan itself made bold is bold.
  expect(within(steps[0]!).getByText('实现迷宫内核').dataset.streamdown).toBe('strong');
  expect(
    within(steps[1]!)
      .getByText(/渲染层/u)
      .closest('[data-streamdown="strong"]'),
  ).toBeNull();
  expect(within(steps[0]!).getByText('验证').className).toContain('agent-plan-label');
  expect(within(steps[1]!).getByText('依赖').nextSibling?.textContent).toBe('S1');
});

test('实施计划 is the card’s main action, and says when it cannot be used', async () => {
  const onImplement = vi.fn();
  const { rerender } = render(<PlanCard plan={plan} disabled={false} onImplement={onImplement} />);
  const button = screen.getByRole('button', { name: '实施计划' });
  expect(button.className).toContain('agent-plan-implement');
  expect(button.querySelector('svg')).not.toBeNull();
  await userEvent.setup().click(button);
  expect(onImplement).toHaveBeenCalledOnce();
  rerender(<PlanCard plan={plan} disabled onImplement={onImplement} />);
  expect(screen.getByRole('button', { name: '实施计划' })).toHaveProperty('disabled', true);
});
