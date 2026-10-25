import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';

import { RuntimeReadinessCard } from './RuntimeReadinessCard';
import { useRuntimeReadiness } from './runtime-readiness-store';

vi.mock('./runtime-readiness-store', () => ({
  useRuntimeReadiness: vi.fn(),
  refreshRuntimeReadiness: vi.fn(),
}));
afterEach(cleanup);

it('S08 distinguishes preparing Shell from ready compatibility without installation repair', () => {
  const value = {
    schema_version: 1 as const,
    revision: '1',
    core_health: 'ready' as const,
    capabilities: [],
    extensions: [],
    backends: [
      {
        backend: 'windows-user-compat-v1',
        lifecycle: 'degraded' as const,
        readiness: { readiness: 'unavailable' as const, reason: 'compat.preparing' },
      },
    ],
  };
  vi.mocked(useRuntimeReadiness).mockReturnValue({ loading: false, value });
  const view = render(<RuntimeReadinessCard />);
  expect(screen.getByText('正在准备 Shell')).toBeTruthy();
  vi.mocked(useRuntimeReadiness).mockReturnValue({
    loading: false,
    value: {
      ...value,
      backends: [
        {
          backend: 'windows-user-compat-v1',
          lifecycle: 'ready',
          readiness: { readiness: 'ready' },
        },
      ],
    },
  });
  view.rerender(<RuntimeReadinessCard />);
  expect(screen.getByText('可运行；目录与网络限制未强制')).toBeTruthy();
  expect(screen.queryByText('需要修复')).toBeNull();
});

it('renders local backend degradation and setup without a global recovery message', () => {
  const value = {
    schema_version: 1 as const,
    revision: '1',
    core_health: 'ready' as const,
    capabilities: [],
    extensions: [],
    backends: [
      {
        backend: 'strict_appcontainer',
        lifecycle: 'degraded' as const,
        readiness: { readiness: 'unavailable' as const, reason: 'backend.start_failed' },
      },
    ],
  };
  vi.mocked(useRuntimeReadiness).mockReturnValue({ loading: false, value });
  const view = render(<RuntimeReadinessCard />);
  expect(screen.getByLabelText('运行能力')).toBeTruthy();
  expect(screen.getByRole('status').textContent).toBe('核心服务可用');
  expect(screen.getByText('暂不可用')).toBeTruthy();
  expect(screen.queryByText('核心数据需要恢复')).toBeNull();
  vi.mocked(useRuntimeReadiness).mockReturnValue({
    loading: false,
    value: {
      ...value,
      backends: [
        {
          backend: 'strict_appcontainer',
          lifecycle: 'ready',
          readiness: { readiness: 'setup_required', reason: 'probe_required' },
        },
      ],
    },
  });
  view.rerender(<RuntimeReadinessCard />);
  expect(screen.getByText('需要环境准备')).toBeTruthy();
});

it('replaces loading and stale status with an accessible transport error', () => {
  vi.mocked(useRuntimeReadiness).mockReturnValue({ loading: true });
  const view = render(<RuntimeReadinessCard />);
  expect(screen.getByRole('status').textContent).toContain('正在检查');
  vi.mocked(useRuntimeReadiness).mockReturnValue({ loading: false, error: '连接暂不可用' });
  view.rerender(<RuntimeReadinessCard />);
  expect(screen.getByRole('status').textContent).toBe('连接暂不可用');
  expect(
    screen.queryByText('模型配置、文件和历史可用。执行能力在使用时检查权限与环境。'),
  ).toBeNull();
});
