import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { App } from './App';
import { installAppTestEnvironment, workspaceSessionProjection } from './app-test-fixtures';

installAppTestEnvironment();
afterEach(cleanup);

// The start page never prints Core's English text for a list it could not read.
test('a workspace list Core did not give reads as words on the start page', async () => {
  vi.mocked(window.aura.v1.getWorkspaceSessions).mockResolvedValue({
    ok: false,
    error: {
      category: 'unavailable',
      code: 'projection.not_ready',
      correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      details: {},
      message: 'Workspace/Session projection is not ready.',
      retryable: true,
      terminal: 'failed',
    },
  });
  const { container } = render(<App />);
  const said =
    '工作区和会话列表还没从 Aura Core 读到（Core 可能还在启动或正忙），列表暂时为空；Aura 会自动再读，长时间仍为空请重启 Aura。';
  await waitFor(() => expect(container.textContent).toContain(said));
  expect(container.textContent).not.toContain('projection is not ready');
  expect(container.textContent?.split(said)).toHaveLength(2);
  expect(screen.getByRole('alert').textContent).toContain('工作区列表暂不可用');
  vi.mocked(window.aura.v1.getWorkspaceSessions).mockResolvedValue({
    ok: true,
    value: workspaceSessionProjection,
  });
});
