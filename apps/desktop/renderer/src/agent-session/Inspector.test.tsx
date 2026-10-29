import type { AgentPromptInspection, PromptInspectorProjection } from '@aura/contracts/generated';
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, test } from 'vitest';

import { Inspector } from './Inspector';

afterEach(cleanup);

const projection = {
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
  cache_usage: { stable_prefix_estimate_tokens: 4_000 },
  compaction: {
    retained_recent_tokens: 0,
    retention_exceptions: [],
    source_context_snapshot_id: null,
    state: 'not_required',
  },
  context_snapshot_id: null,
  context_usage: null,
  model_egress: null,
  model_profile_id: null,
  overflow: null,
  prompt_bundle_snapshot_id: null,
  revision: '7',
  sections: [],
  session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  skills: [],
  tools: [],
} satisfies PromptInspectorProjection;

const section = (kind: string, tokens: number, changed: boolean, ordinal: number) => ({
  byte_count: tokens * 4,
  changed_from_previous_step: changed,
  character_count: tokens * 3,
  compressibility: 'fixed',
  content_hash: { algorithm: 'sha256', hex: 'ab'.repeat(32) },
  estimated_tokens: tokens,
  is_empty_marker: false,
  kind,
  physical_ordinal: ordinal,
  placement: 'prefix',
  privacy_class: 'model_visible',
  source_kinds: ['aura_core'],
});
const tool = (name: string, tokens: number) => ({
  tool_id: `tol_${name}`,
  canonical_name: name,
  schema_hash: { algorithm: 'sha256', hex: 'cd'.repeat(32) },
  estimated_tokens: tokens,
});

function inspection(
  cache: PromptInspectorProjection['cache_usage'],
  more: Partial<PromptInspectorProjection> = {},
): AgentPromptInspection {
  return {
    projection: { ...projection, cache_usage: cache, ...more },
    execution: {
      model_name: 'gpt-6.1-sol',
      api_format: 'open_ai_responses',
      model_config_revision: 36,
      model_step_id: 'mst_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      prompt_bundle_snapshot_id: 'pbs_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    },
    context: {
      id: 'cps_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      tool_schema_set: { registry_snapshot_ref: { tool_registry_snapshot_id: 'trs_01' } },
    },
  } as unknown as AgentPromptInspection;
}

describe('Inspector', () => {
  test("names the session behind each other task the step's task state numbered", () => {
    const view = inspection({ stable_prefix_estimate_tokens: 4_000 });
    view.activity_sources = [
      { another_task: 1, session_id: 'ses_aioj', workspace_id: 'wsp_aioj' },
      { another_task: 2, session_id: 'ses_gone', workspace_id: 'wsp_gone' },
    ];
    render(
      <Inspector
        view={view}
        error={undefined}
        workspaces={[
          {
            id: 'wsp_aioj',
            name: 'aioj',
            root: 'C:/Dev/aioj',
            sessions: [{ id: 'ses_aioj', state: 'running', title: '启动 Docker' }],
          },
        ]}
      />,
    );
    const others = screen.getByRole('region', { name: '其它任务' });
    expect(others.textContent).toContain('任务 1');
    expect(others.textContent).toContain('aioj › 启动 Docker');
    expect(others.textContent).toContain('未知工作区 › 未知会话');
  });

  // The provider's own count of the step's input, beside the part of
  // it the provider's cache supplied; as a share and a pixel bar.
  test('heads the panel with the model and its format, and shows the cache hit as a share', () => {
    render(
      <Inspector
        view={inspection({
          actual_input_tokens: 128_400,
          actual_cached_input_tokens: 121_600,
          stable_prefix_estimate_tokens: 4_000,
        })}
        error={undefined}
      />,
    );
    expect(screen.getByText('gpt-6.1-sol')).toBeTruthy();
    expect(screen.getByText('Responses')).toBeTruthy();
    const cache = screen.getByRole('region', { name: '缓存' });
    expect(cache.textContent).toContain('94.70%');
    expect(within(cache).getByRole('img', { name: '缓存 121,600 / 128,400' })).toBeTruthy();
  });

  test("leads with the whole conversation's cache share, and says this step's below it", () => {
    const view = inspection({ stable_prefix_estimate_tokens: 4_000 });
    view.session_cache = {
      steps: 53,
      reported_steps: 52,
      input_tokens: 5_113_289,
      cached_input_tokens: 4_615_168,
    };
    render(<Inspector view={view} error={undefined} />);
    const cache = screen.getByRole('region', { name: '缓存' });
    expect(cache.textContent).toContain('90.26%');
    expect(within(cache).getByRole('img', { name: '会话缓存 4,615,168 / 5,113,289' })).toBeTruthy();
    expect(cache.textContent).toContain('52 次请求');
    // The step shown is still running: it has no figures of its own yet.
    expect(cache.textContent).toContain('本步 未完成');
  });

  test("shows this step's share beside the conversation's once it finished", () => {
    const view = inspection({
      actual_input_tokens: 128_400,
      actual_cached_input_tokens: 121_600,
      stable_prefix_estimate_tokens: 4_000,
    });
    view.session_cache = {
      steps: 2,
      reported_steps: 2,
      input_tokens: 200_000,
      cached_input_tokens: 150_000,
    };
    render(<Inspector view={view} error={undefined} />);
    const cache = screen.getByRole('region', { name: '缓存' });
    expect(cache.textContent).toContain('75.00%');
    expect(cache.textContent).toContain('本步 94.70%');
  });

  test('says when the provider reported neither', () => {
    render(
      <Inspector view={inspection({ stable_prefix_estimate_tokens: 4_000 })} error={undefined} />,
    );
    const cache = screen.getByRole('region', { name: '缓存' });
    expect(cache.textContent).toContain('未报告');
    expect(within(cache).queryByRole('img')).toBeNull();
  });

  test('draws modules and the largest tools as bars; identities wait in the details', () => {
    const { container } = render(
      <Inspector
        view={inspection(
          { stable_prefix_estimate_tokens: 4_000 },
          {
            sections: [
              section('core_system_prompt', 2_126, false, 1),
              section('recent_history', 55_556, true, 9),
            ] as unknown as PromptInspectorProjection['sections'],
            tools: [
              tool('fs.write', 812),
              tool('fs.read', 640),
              tool('shell.powershell', 1_204),
              tool('git.diff', 300),
              tool('web.fetch', 280),
              tool('todo.update', 210),
              tool('memory.read', 120),
            ] as unknown as PromptInspectorProjection['tools'],
          },
        )}
        error={undefined}
      />,
    );
    const modules = screen.getByRole('region', { name: '模块' });
    expect(modules.textContent).toContain('系统提示');
    expect(modules.textContent).toContain('2,126');
    expect(modules.textContent).toContain('近期对话');
    expect(modules.querySelectorAll('.pixel-changed')).toHaveLength(1);
    const tools = screen.getByRole('region', { name: '工具' });
    expect(tools.textContent).toContain('shell.powershell');
    expect(tools.textContent).not.toContain('memory.read');
    expect(tools.textContent).toContain('其余 1 个');
    const details = container.querySelector('details') as HTMLDetailsElement;
    expect(details.open).toBe(false);
    // Nothing of the snapshot is laid out until its details open.
    expect(details.textContent).toBe('快照元数据');
    act(() => {
      details.open = true;
      fireEvent(details, new Event('toggle'));
    });
    expect(details.textContent).toContain('mst_01ARZ3NDEKTSV4RRFFQ69G5FAV');
    const outside = [...container.querySelectorAll('.pixel-card')].map((card) => card.textContent);
    expect(outside.join('')).not.toContain('sha256');
    expect(outside.join('')).not.toContain('mst_01ARZ3NDEKTSV4RRFFQ69G5FAV');
  });

  // Only the newest full snapshots of a conversation are kept; an older step's details still
  // show its figures, from the summary every step keeps.
  test('an older step without its full snapshot still shows its figures and its identifiers', () => {
    const view = inspection(
      { stable_prefix_estimate_tokens: 4_000 },
      {
        sections: [
          section('core_system_prompt', 2_126, false, 1),
        ] as unknown as PromptInspectorProjection['sections'],
      },
    );
    delete (view as Partial<AgentPromptInspection>).context;
    const { container } = render(<Inspector view={view} error={undefined} />);
    expect(screen.getByRole('region', { name: '模块' }).textContent).toContain('2,126');
    const details = container.querySelector('details') as HTMLDetailsElement;
    act(() => {
      details.open = true;
      fireEvent(details, new Event('toggle'));
    });
    expect(details.textContent).toContain('mst_01ARZ3NDEKTSV4RRFFQ69G5FAV');
    expect(container.querySelector('.snapshot-json')).toBeNull();
  });
});
