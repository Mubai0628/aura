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
});
