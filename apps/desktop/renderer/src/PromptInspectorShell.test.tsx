import type { PromptInspectorProjection } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, test } from 'vitest';

import { PromptInspectorShell } from './PromptInspectorShell';

afterEach(cleanup);

const projection: PromptInspectorProjection = {
  active_agent_run_id: null,
  active_model_step_id: null,
  active_task_lineage: null,
  active_tool_call_ids: [],
  attachments: [],
  budget: {
    estimated_input_tokens: 10_000,
    estimator_id: 'utf8-byte-upper-bound',
    estimator_version: 1,
    exact: false,
    output_reserve_tokens: 8_192,
    policy: 'context_budget_policy_v1',
    request_input_budget_tokens: 53_248,
    runtime_effective_window_tokens: 65_536,
    safety_reserve_tokens: 4_096,
  },
  cache_usage: { actual_cached_input_tokens: null, stable_prefix_estimate_tokens: 4_000 },
  compaction: {
    retained_recent_tokens: 0,
    retention_exceptions: [],
    source_context_snapshot_id: null,
    state: 'not_required',
  },
  context_snapshot_id: null,
  context_usage: {
    estimated: true,
    runtime_effective_window_tokens: 65_536,
    segments: [
      { estimated_tokens: 4_000, kind: 'system_and_rules', label: '系统与规则' },
      { estimated_tokens: 1_000, kind: 'tools_and_results', label: '工具与结果' },
      { estimated_tokens: 3_000, kind: 'conversation', label: '对话内容' },
      {
        estimated_tokens: 2_000,
        kind: 'background_and_compaction',
        label: '背景与压缩历史',
      },
    ],
    used_input_tokens: 10_000,
  },
  model_egress: null,
  model_profile_id: null,
  overflow: null,
  prompt_bundle_snapshot_id: null,
  revision: '7',
  sections: [],
  session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  skills: [],
  tools: [],
};

//
describe('PromptInspectorShell', () => {
  test('draws the context and the budget as pixel bars with their numbers, and no notes', () => {
    render(<PromptInspectorShell projection={projection} />);
    const shell = screen.getByRole('region', { name: '提示词监测' });
    const context = screen.getByRole('img', { name: '上下文 10,000 / 65,536' });
    expect(context.querySelectorAll('.pixel-cell')).toHaveLength(32);
    expect(context.querySelectorAll('.pixel-cell.is-system_and_rules').length).toBeGreaterThan(0);
    expect(context.querySelectorAll('.pixel-cell.is-free').length).toBeGreaterThan(0);
    expect(shell.textContent).toContain('15.26%');
    expect(screen.getByText('系统与规则').closest('li')?.textContent).toContain('4,000');
    const budget = screen.getByRole('img', { name: /^预算/u });
    expect(budget.querySelectorAll('.pixel-cell.is-output').length).toBeGreaterThan(0);
    expect(shell.textContent).toContain('53,248');
    expect(shell.textContent).toContain('无需压缩');
    expect(shell.textContent).not.toContain('展示所选模型步骤的已提交快照');
    expect(shell.textContent).not.toContain('Revision');
    expect(shell.textContent).not.toContain('prompt text');
  });

  test('shows a waiting state without inventing usage', () => {
    render(<PromptInspectorShell projection={undefined} />);
    expect(screen.getByText('尚无已提交的上下文快照')).toBeDefined();
    expect(screen.queryByRole('img')).toBeNull();
  });
});
