import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { ContextUsageRing } from './ContextUsageRing';
afterEach(cleanup);
const usage = {
  estimated: true,
  runtime_effective_window_tokens: 1000,
  used_input_tokens: 300,
  segments: [
    { kind: 'system_and_rules' as const, label: '系统与规则', estimated_tokens: 100 },
    { kind: 'conversation' as const, label: '对话', estimated_tokens: 200 },
  ],
};
test('shows an empty fixed ring and clamps overflow without invented tokens', () => {
  const { rerender } = render(<ContextUsageRing usage={undefined} onOpenInspector={vi.fn()} />);
  expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe('0');
  rerender(
    <ContextUsageRing usage={{ ...usage, used_input_tokens: 1200 }} onOpenInspector={vi.fn()} />,
  );
  expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe('100');
});
test('click opens layered real usage; Escape dismisses; inspector opens only from its own action', async () => {
  const user = userEvent.setup();
  const inspect = vi.fn();
  render(<ContextUsageRing usage={usage} onOpenInspector={inspect} />);
  await user.click(screen.getByRole('button', { name: /上下文占用/u }));
  expect(screen.getByRole('region', { name: '上下文占用详情' }).textContent).toContain(
    '系统与规则',
  );
  expect(inspect).not.toHaveBeenCalled();
  await user.keyboard('{Escape}');
  expect(screen.queryByRole('region', { name: '上下文占用详情' })).toBeNull();
  await user.click(screen.getByRole('button', { name: /上下文占用/u }));
  await user.click(screen.getByRole('button', { name: /检查上下文详情/u }));
  expect(inspect).toHaveBeenCalledOnce();
});
test('an older request is labelled instead of passing for the current step', async () => {
  const user = userEvent.setup();
  render(
    <ContextUsageRing
      source={{ kind: 'recent', modelStepId: 'stp_previous' }}
      usage={usage}
      onOpenInspector={vi.fn()}
    />,
  );
  await user.click(screen.getByRole('button', { name: /上下文占用/u }));
  expect(screen.getByRole('note').textContent).toContain('最近一次真实模型请求');
});
test('distinguishes no request yet from a failed read', async () => {
  const user = userEvent.setup();
  const { rerender } = render(
    <ContextUsageRing source={{ kind: 'none' }} usage={undefined} onOpenInspector={vi.fn()} />,
  );
  const button = () => screen.getByRole('button', { name: /上下文占用/u });
  expect(button().getAttribute('aria-label')).toContain('尚无模型请求');
  await user.click(button());
  expect(screen.getByRole('note').textContent).toContain('还没有形成模型请求');
  await user.keyboard('{Escape}');
  rerender(
    <ContextUsageRing source={{ kind: 'failed' }} usage={undefined} onOpenInspector={vi.fn()} />,
  );
  expect(button().getAttribute('aria-label')).toContain('读取失败');
});
