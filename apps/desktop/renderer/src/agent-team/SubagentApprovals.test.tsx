import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ACTIVITY_EVERY_MS } from '../session-activity';
import { SubagentApprovals } from './SubagentApprovals';
import { resetAgentTeamsForTests } from './team-store';

// The approvals a conversation's sub-agents wait for are decided in that conversation, each
// marked with the sub-agent that asked, and read again while there is work under way.

const list = vi.fn();
const getApproval = vi.fn();
beforeEach(() => {
  vi.useFakeTimers();
  list.mockReset();
  getApproval.mockReset();
  getApproval.mockReturnValue(new Promise(() => {}));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { listAgentTeam: list, getAgentApproval: getApproval } },
  });
});
afterEach(() => {
  cleanup();
  resetAgentTeamsForTests();
  vi.useRealTimers();
});

const member = (agent: string, name: string, approvals: string[], state = 'running') => ({
  agent,
  name,
  session_id: `ses_${agent}`,
  model_profile_id: 'mpr_flash',
  model: 'DeepSeek V4.1 Flash',
  state,
  working: state === 'running',
  approvals,
  tokens: 0,
});

test('a sub-agent waiting for an approval shows it here, marked with its name', async () => {
  list.mockResolvedValue({
    ok: true,
    value: { members: [member('A1', '读文档', ['apr_1']), member('A2', '查测试', [])] },
  });
  render(<SubagentApprovals sessionId="ses_main" activeTurn="trn_1" />);
  await act(async () => {});
  expect(list).toHaveBeenCalledWith({ session_id: 'ses_main' });
  expect(screen.getAllByText('子 Agent · 读文档')).toHaveLength(1);
  expect(screen.queryByText('子 Agent · 查测试')).toBeNull();
  expect(getApproval).toHaveBeenCalledWith({ session_id: 'ses_A1', approval_id: 'apr_1' });
});

test('it reads again while work is under way and stops once nothing runs', async () => {
  list.mockResolvedValue({ ok: true, value: { members: [member('A1', '读文档', [])] } });
  const { rerender } = render(<SubagentApprovals sessionId="ses_main" activeTurn="trn_1" />);
  await act(async () => {});
  await act(async () => vi.advanceTimersByTime(ACTIVITY_EVERY_MS));
  expect(list).toHaveBeenCalledTimes(2);
  list.mockResolvedValue({
    ok: true,
    value: { members: [member('A1', '读文档', [], 'reported')] },
  });
  rerender(<SubagentApprovals sessionId="ses_main" />);
  // The next read finds nothing at work; after it nothing is read again.
  await act(async () => vi.advanceTimersByTime(ACTIVITY_EVERY_MS));
  await act(async () => {});
  const asked = list.mock.calls.length;
  await act(async () => vi.advanceTimersByTime(ACTIVITY_EVERY_MS * 4));
  expect(list).toHaveBeenCalledTimes(asked);
});

test('a conversation without sub-agents shows nothing', async () => {
  list.mockResolvedValue({ ok: true, value: { members: [] } });
  const { container } = render(<SubagentApprovals sessionId="ses_main" />);
  await act(async () => {});
  expect(container.textContent).toBe('');
});
