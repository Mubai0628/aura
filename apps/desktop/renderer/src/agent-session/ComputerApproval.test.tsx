import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { ApprovalPanel } from './ApprovalPanel';

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const NOTEPAD = 'Microsoft.WindowsNotepad_8wekyb3d8bbwe!App';
const resolveAgentApproval = vi.fn(async () => ({ ok: true, value: {} }));
const alwaysAllow = vi.fn(async () => ({ ok: true, value: {} }));
const icons = vi.fn(async () => ({
  ok: true,
  value: { icons: [{ app_id: NOTEPAD, png: 'iVBORw0KGgo=' }] },
}));

/** A call on Notepad as Core resolved it: the host's command names the program. */
function install(action: string, command: Record<string, unknown>) {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getAgentApproval: async () => ({
          ok: true,
          value: {
            mode: 'workspace_read_write',
            backend: { id: 'agent-computer-v1', class: 'core_broker', guarantees: [] },
            operation: {
              content: {
                action_kind: action,
                action: { command },
                purpose: '在记事本里写一段话',
                resources: [{ scope_kind: 'desktop_app', app_id: NOTEPAD, authorized: false }],
                capabilities: ['desktop_input'],
                credentials: [],
              },
            },
            requested_capabilities: ['desktop_input'],
            reasons: ['policy.approval_required'],
            approval: {
              revision: 4,
              expires_at: '2026-11-02T00:00:00Z',
              state: 'pending',
              allowed_decisions: ['allow_session', 'deny'],
            },
          },
        }),
        resolveAgentApproval,
        computer: { alwaysAllow, icons },
      },
    },
  });
}

const app = { id: NOTEPAD, name: '记事本', kind: 'packaged' };

test('a call on a program names it on the card with its icon and offers 本会话允许, 总是允许 and 拒绝', async () => {
  install('computer.act', { op: 'act', app, window: 'w1', steps: [], timeout_ms: 20000 });
  render(<ApprovalPanel sessionId="ses" approvalId="apr" onChanged={() => undefined} />);
  await screen.findByRole('region', { name: '电脑操控' });
  expect(screen.getByText('操作程序')).toBeTruthy();
  // On the card itself, not behind 查看详情.
  const program = screen.getByText('记事本');
  expect(program.closest('.agent-approval-program')).toBeTruthy();
  expect(program.closest('details')).toBeNull();
  expect(program.getAttribute('title')).toBe(NOTEPAD);
  await vi.waitFor(() =>
    expect(program.querySelector('img')?.getAttribute('src')).toBe(
      'data:image/png;base64,iVBORw0KGgo=',
    ),
  );
  expect(
    screen
      .getAllByRole('button')
      .map((button) => button.textContent)
      .filter((text) => ['本会话允许', '总是允许', '拒绝'].includes(text ?? '')),
  ).toEqual(['本会话允许', '总是允许', '拒绝']);
});

test('总是允许 puts the program on the list, then answers for this session', async () => {
  install('computer.screenshot', {
    op: 'screenshot',
    show: [app],
    paint: true,
    max_long_edge: 1568,
  });
  render(<ApprovalPanel sessionId="ses" approvalId="apr" onChanged={() => undefined} />);
  await userEvent.click(await screen.findByRole('button', { name: '总是允许' }));
  expect(alwaysAllow).toHaveBeenCalledWith({ app_id: NOTEPAD, name: '记事本' });
  await vi.waitFor(() =>
    expect(resolveAgentApproval).toHaveBeenCalledWith({
      session_id: 'ses',
      approval_id: 'apr',
      expected_revision: 4,
      decision: 'allow_session',
    }),
  );
  expect(alwaysAllow.mock.invocationCallOrder[0]).toBeLessThan(
    resolveAgentApproval.mock.invocationCallOrder[0]!,
  );
});

test('a list that was not saved answers nothing and says why', async () => {
  alwaysAllow.mockResolvedValueOnce({
    ok: false,
    error: {
      code: 'computer_settings.not_saved',
      message: 'computer_settings.not_saved: 名单已满',
    },
  } as never);
  install('computer.open', { op: 'launch', app, timeout_ms: 15000 });
  render(<ApprovalPanel sessionId="ses" approvalId="apr" onChanged={() => undefined} />);
  await userEvent.click(await screen.findByRole('button', { name: '总是允许' }));
  expect((await screen.findByRole('alert')).textContent).toContain('名单已满');
  expect(resolveAgentApproval).not.toHaveBeenCalled();
});
