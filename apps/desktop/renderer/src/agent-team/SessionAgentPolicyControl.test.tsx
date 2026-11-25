import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { AgentSessionPolicySetRequest } from '@aura/contracts/generated';
import { SessionAgentPolicyControl } from './SessionAgentPolicyControl';

const session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const other = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW';
const value = (id = session, allowed = false, revision = 0) => ({
  session_id: id,
  allow_proactive_temporary_agents: allowed,
  revision,
});
const read = vi.fn();
const save = vi.fn();
beforeEach(() => {
  read.mockReset().mockResolvedValue({ ok: true, value: value() });
  save.mockReset().mockImplementation(async (request: AgentSessionPolicySetRequest) => ({
    ok: true,
    value: value(
      request.session_id,
      request.allow_proactive_temporary_agents,
      request.expected_revision + 1,
    ),
  }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readSessionAgentPolicy: read, setSessionAgentPolicy: save } },
  });
});
afterEach(cleanup);

test('defaults off and allows a keyboard change at the read revision', async () => {
  const user = userEvent.setup();
  render(<SessionAgentPolicyControl sessionId={session} />);
  const control = await screen.findByRole('switch', { name: '允许子 Agent' });
  await waitFor(() => expect(control).toHaveProperty('disabled', false));
  expect(control.getAttribute('aria-checked')).toBe('false');
  await user.tab();
  await user.keyboard(' ');
  await waitFor(() =>
    expect(save).toHaveBeenCalledWith({
      session_id: session,
      expected_revision: 0,
      allow_proactive_temporary_agents: true,
    }),
  );
  await waitFor(() => expect(control.getAttribute('aria-checked')).toBe('true'));
});

test('an unknown write is never replayed and needs an explicit read before another change', async () => {
  const user = userEvent.setup();
  save.mockRejectedValueOnce(new Error('transport lost'));
  render(<SessionAgentPolicyControl sessionId={session} />);
  const control = await screen.findByRole('switch', { name: '允许子 Agent' });
  await waitFor(() => expect(control).toHaveProperty('disabled', false));
  await user.click(control);
  expect((await screen.findByRole('status')).textContent).toContain('尚未确认');
  await user.click(control);
  expect(save).toHaveBeenCalledTimes(1);
  read.mockResolvedValue({ ok: true, value: value(session, true, 1) });
  await user.click(screen.getByRole('button', { name: '重新读取策略' }));
  await waitFor(() => expect(control.getAttribute('aria-checked')).toBe('true'));
  expect(save).toHaveBeenCalledTimes(1);
});

test('a late old-session read cannot set another conversations preference', async () => {
  let release: ((result: unknown) => void) | undefined;
  read.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        release = resolve;
      }),
  );
  const { rerender } = render(<SessionAgentPolicyControl sessionId={session} />);
  read.mockResolvedValue({ ok: true, value: value(other, true, 4) });
  rerender(<SessionAgentPolicyControl sessionId={other} />);
  const control = await screen.findByRole('switch', { name: '允许子 Agent' });
  await waitFor(() => expect(control.getAttribute('aria-checked')).toBe('true'));
  release?.({ ok: true, value: value() });
  await waitFor(() => expect(control.getAttribute('aria-checked')).toBe('true'));
  expect(save).not.toHaveBeenCalled();
});
