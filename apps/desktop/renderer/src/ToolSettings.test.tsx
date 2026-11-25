import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

import { ToolSettings } from './ToolSettings';
import { resetScopedToolProjectionStoreForTests } from './tool-projection-store';

describe('ToolSettings', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'aura', {
      configurable: true,
      value: {
        v1: {
          packages: {
            invoke: vi.fn().mockResolvedValue({ ok: true, value: { packages: [] } }),
          },
          git: {
            runtime: vi.fn().mockResolvedValue({
              ok: true,
              value: { state: 'available', message: '本机 Git 可用', next_step: '可重新检测' },
            }),
          },
          readAgentToolSettings: vi.fn().mockResolvedValue({
            ok: true,
            value: {
              revision: '1',
              fixed_tool_ids: [],
              max_dynamic_tool_schemas: 50,
              max_concurrent_tool_calls: 4,
              default_timeout_ms: 120_000,
              max_retries: 3,
              tools: [],
            },
          }),
          onToolProjection: vi.fn(() => vi.fn()),
          refreshToolProjection: vi.fn().mockResolvedValue({
            error: { code: 'tool_projection.unavailable', message: 'no committed value' },
            ok: false,
          }),
          subscribeToolProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
          unsubscribeToolProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
        },
      },
    });
  });

  afterEach(() => {
    cleanup();
    resetScopedToolProjectionStoreForTests();
    vi.restoreAllMocks();
  });

  test('offers tool packages on their own page section and no retired Draft views', async () => {
    render(<ToolSettings />);
    expect(screen.getByRole('heading', { level: 2, name: '工具与扩展' })).toBeTruthy();
    expect(await screen.findByRole('heading', { name: '工具包' })).toBeTruthy();
    expect(screen.queryByText('Draft 与审批')).toBeNull();
    expect(screen.queryByText('已安装工具包')).toBeNull();
    expect(screen.queryByRole('button', { name: '校验 Draft' })).toBeNull();
    // No explanatory notice on the page, and Git moved to 设置 → 环境.
    expect(screen.queryByText(/新的调用会被拒绝；正在执行的调用允许完成/)).toBeNull();
    expect(screen.queryByRole('region', { name: 'Git 运行环境' })).toBeNull();
  });

  test('has no proxy of its own: tools follow the system or the proxy on the model page', async () => {
    render(<ToolSettings />);
    expect(await screen.findByRole('heading', { name: '执行策略' })).toBeTruthy();
    expect(screen.queryByRole('region', { name: '网络代理' })).toBeNull();
    expect(screen.queryByRole('switch', { name: '启用网络代理' })).toBeNull();
    expect(screen.queryByRole('switch', { name: '跟随系统代理' })).toBeNull();
  });
});

describe('ToolSettings when its settings are not read', () => {
  const issue = {
    payload: {
      configuration_issue: {
        category: 'conflict',
        code: 'tools.settings_invalid',
        message: '工具设置无效；Aura 已保留最近一次有效配置。',
        retryable: false,
      },
    },
    projection_kind: 'tool_settings',
    projection_scope: { type: 'global' },
    revision: '1',
    runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  };

  const stub = (v1: Record<string, unknown>) =>
    Object.defineProperty(window, 'aura', {
      configurable: true,
      value: {
        v1: {
          packages: {
            invoke: vi.fn().mockResolvedValue({ ok: true, value: { packages: [] } }),
          },
          readAgentToolSettings: vi.fn().mockResolvedValue({
            ok: true,
            value: {
              revision: '1',
              fixed_tool_ids: [],
              max_dynamic_tool_schemas: 50,
              max_concurrent_tool_calls: 4,
              default_timeout_ms: 120_000,
              max_retries: 3,
              tools: [],
            },
          }),
          onToolProjection: vi.fn(() => vi.fn()),
          refreshToolProjection: vi.fn().mockResolvedValue({
            error: { code: 'tool_projection.unavailable', message: 'no committed value' },
            ok: false,
          }),
          subscribeToolProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
          unsubscribeToolProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
          ...v1,
        },
      },
    });

  afterEach(() => {
    cleanup();
    resetScopedToolProjectionStoreForTests();
  });

  test('a settings snapshot that cannot be subscribed says why and that it is retried', async () => {
    stub({
      subscribeToolProjection: vi.fn().mockResolvedValue({
        ok: false,
        error: {
          code: 'ipc.core_not_ready',
          message: 'Aura Core 尚未就绪，这次操作没有执行；请等它就绪后重试。',
        },
      }),
    });
    render(<ToolSettings />);
    expect(
      await screen.findByText(
        '工具设置还没有读到（Aura Core 还没有就绪），正在重试；一直这样请重启 Aura。',
      ),
    ).toBeTruthy();
    expect(screen.queryByText(/Tool Settings/u)).toBeNull();
  });

  test('settings rejected from outside show the diagnostic and how to recover', async () => {
    stub({ refreshToolProjection: vi.fn().mockResolvedValue({ ok: true, value: issue }) });
    render(<ToolSettings />);
    const banner = await screen.findByRole('alert');
    expect(banner.textContent).toContain('工具设置文件里的改动无效');
    expect(banner.textContent).toContain(issue.payload.configuration_issue.message);
    expect(banner.textContent).toContain(
      'Aura 正在使用上次有效的设置；在本页改好后保存即可覆盖它。',
    );
  });
});
