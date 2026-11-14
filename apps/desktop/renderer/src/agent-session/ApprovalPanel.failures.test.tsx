import type { ExecutionApprovalView } from '@aura/contracts/generated';
import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { ApprovalPanel } from './ApprovalPanel';

// An approval card whose read failed says why and that the step still waits, and reads again
// when Aura's connection to Core was why. Its details name how the call runs in words.

const value = {
  approval: {
    approval_id: 'apr_1',
    revision: '1',
    state: 'pending',
    requested_at: '2026-09-25T00:00:00Z',
    expires_at: '2026-09-25T00:05:00Z',
    allowed_decisions: ['allow_once', 'deny'],
    scope: {
      lifetime: { lifetime: 'once', operation_id: 'op_1' },
      profile: 'balanced',
      operation_fingerprint: 'hash',
      backend_requirement: { allowed_classes: ['native'], guarantees: [] },
    },
  },
  operation: {
    content: {
      action_kind: 'web.fetch',
      action: { url: 'https://example.com' },
      capabilities: ['public_network_read'],
      resources: [],
      purpose: '读取网页',
    },
  },
  mode: 'workspace_read_write',
  backend: {
    id: 'windows-user-native-v1',
    class: 'native',
    guarantees: ['bounded_output', 'child_process_control'],
  },
  requested_capabilities: ['process_execute', 'ordinary_network'],
  reasons: ['policy.approval_required', 'backend.native_explicit', 'something.unknown'],
} as unknown as ExecutionApprovalView;

const install = (getAgentApproval: () => Promise<unknown>) =>
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { getAgentApproval, resolveAgentApproval: vi.fn() } },
  });

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

test('a card Core did not answer says the step still waits, and reads again', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const read = vi
    .fn()
    .mockResolvedValueOnce({
      ok: false,
      error: { code: 'ipc.request_timeout', message: 'Core request timed out' },
    })
    .mockResolvedValueOnce({ ok: true, value });
  install(read);
  render(<ApprovalPanel sessionId="ses" approvalId="apr_1" onChanged={() => undefined} />);
  expect(await screen.findByRole('alert')).toHaveProperty(
    'textContent',
    '审批内容没有读到（Aura Core 没有及时回复），这一步仍在等你批准；会自动重新读取。',
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5_000);
  });
  expect(await screen.findByRole('button', { name: '仅此一次' })).toBeTruthy();
  expect(screen.queryByRole('alert')).toBeNull();
  expect(read).toHaveBeenCalledTimes(2);
});

test('a card Core refused to read says what to do', async () => {
  install(async () => ({
    ok: false,
    error: { code: 'agent.approval_missing', message: 'approval was not found' },
  }));
  render(<ApprovalPanel sessionId="ses" approvalId="apr_1" onChanged={() => undefined} />);
  expect(await screen.findByRole('alert')).toHaveProperty(
    'textContent',
    '审批内容没有读到，这一步仍在等你批准；请稍后重新打开这个会话。',
  );
});

test('the details name how the call runs in words, without a JSON dump', async () => {
  install(async () => ({ ok: true, value }));
  const { container } = render(
    <ApprovalPanel sessionId="ses" approvalId="apr_1" onChanged={() => undefined} />,
  );
  await screen.findByRole('button', { name: '仅此一次' });
  const technical = container.querySelector('.agent-approval-technical')!;
  const facts = Object.fromEntries(
    [...technical.querySelectorAll('dt')].map((term) => [
      term.textContent,
      term.nextElementSibling?.textContent,
    ]),
  );
  expect(facts).toMatchObject({
    本次执行方式: '本机执行环境',
    隔离保障: '输出有上限、子进程受控',
    本次请求批准的能力: '运行程序、联网',
  });
  expect(container.querySelector('pre')).toBeNull();
  expect(container.querySelector('.agent-approval-reasons')?.textContent).toBe(
    '需要确认的原因当前权限模式要求确认这次以你的 Windows 用户身份直接运行',
  );
  expect(container.textContent).not.toMatch(/windows-user-native-v1|bounded_output|policy\./u);
});
