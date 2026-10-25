import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { ContextUsageRing } from './ContextUsageRing';

// Each kind of use is a row of squares in its own colour, as much
// of the window as it takes; the last row is the whole use. Shares keep two decimals.

afterEach(cleanup);

const usage = {
  estimated: true,
  runtime_effective_window_tokens: 1_000_000,
  used_input_tokens: 292_000,
  segments: [
    { kind: 'system_and_rules' as const, label: '系统与规则', estimated_tokens: 9_423 },
    { kind: 'tools_and_results' as const, label: '工具与结果', estimated_tokens: 182_000 },
    { kind: 'conversation' as const, label: '对话内容', estimated_tokens: 100_000 },
    { kind: 'background_and_compaction' as const, label: '背景与压缩历史', estimated_tokens: 224 },
  ],
};

async function opened(props: Partial<Parameters<typeof ContextUsageRing>[0]> = {}) {
  const user = userEvent.setup();
  const inspect = vi.fn();
  render(<ContextUsageRing usage={usage} onOpenInspector={inspect} {...props} />);
  await user.click(screen.getByRole('button', { name: /上下文占用/u }));
  return { panel: screen.getByRole('region', { name: '上下文占用详情' }), inspect, user };
}

const rows = (panel: HTMLElement) =>
  within(panel)
    .getAllByRole('listitem')
    .map((row) => ({
      name: row.querySelector('.pixel-row-name')?.textContent,
      share: row.querySelector('b')?.textContent,
      kinds: [
        ...new Set(
          [...row.querySelectorAll('.pixel-cell')]
            .map((cell) => [...cell.classList].find((name) => name.startsWith('is-')))
            .filter((name) => name !== 'is-free'),
        ),
      ],
    }));

test('each kind of use is a row of its own colour, and the last row is the whole use', async () => {
  const { panel } = await opened();
  expect(rows(panel)).toEqual([
    { name: '系统与规则', share: '0.94%', kinds: ['is-system_and_rules'] },
    { name: '工具与结果', share: '18.20%', kinds: ['is-tools_and_results'] },
    { name: '对话内容', share: '10.00%', kinds: ['is-conversation'] },
    { name: '背景与压缩历史', share: '0.02%', kinds: ['is-background_and_compaction'] },
    {
      name: '总占用',
      share: '29.20%',
      kinds: [
        'is-system_and_rules',
        'is-tools_and_results',
        'is-conversation',
        'is-background_and_compaction',
      ],
    },
  ]);
  // A row's squares are its share of the window: 18.2% of the row is filled for tools and results.
  const tools = within(panel).getAllByRole('listitem')[1]!;
  const cells = tools.querySelectorAll('.pixel-cell');
  const filled = tools.querySelectorAll('.pixel-cell.is-tools_and_results');
  expect(filled.length / cells.length).toBeCloseTo(0.182, 1);
});

test('a row names its amount and its share of what is used', async () => {
  const { panel } = await opened();
  const tools = within(panel).getAllByRole('listitem')[1]!;
  expect(tools.getAttribute('title')).toBe('工具与结果 · 18.2万 · 占已用 62.40%');
  const total = within(panel).getAllByRole('listitem')[4]!;
  expect(total.getAttribute('title')).toBe('估算：约 29.2万 / 100万 tokens');
});

test('where the numbers come from, and a compaction under way, are said', async () => {
  const { panel } = await opened({
    source: { kind: 'recent', modelStepId: 'stp_previous' },
    running: true,
  });
  expect(within(panel).getByRole('status').textContent).toBe('压缩中');
  expect(within(panel).getByRole('note').textContent).toContain('最近一次真实模型请求');
});

test('↗ opens the context details', async () => {
  const { inspect, user } = await opened();
  await user.click(screen.getByRole('button', { name: '检查上下文详情' }));
  expect(inspect).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole('region', { name: '上下文占用详情' })).toBeNull();
});
