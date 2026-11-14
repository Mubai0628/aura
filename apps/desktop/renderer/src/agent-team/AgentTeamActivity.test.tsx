import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { AgentTeamActivity } from './AgentTeamActivity';
import { resetAgentTeamsForTests } from './team-store';

// The 近期活动 card says how many sub-agents work and how many need the user, and opens the
// 多 Agent page.

const list = vi.fn();
beforeEach(() => {
  list.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { listAgentTeam: list } },
  });
});
afterEach(() => {
  cleanup();
  resetAgentTeamsForTests();
});

const member = (agent: string, state: string, more: Record<string, unknown> = {}) => ({
  agent,
  name: agent,
  session_id: `ses_${agent}`,
  model_profile_id: 'mpr_flash',
  model: 'Flash',
  state,
  working: state === 'running',
  approvals: [],
  tokens: 0,
  ...more,
});
const team = (...members: unknown[]) => ({
  ok: true,
  value: { lead: { working: true, tokens: 0 }, members },
});

test('it counts the sub-agents at work and those that need the user, and opens the page', async () => {
  list.mockResolvedValue(
    team(
      member('A1', 'running'),
      member('A2', 'running', { approvals: ['apr_1'] }),
      member('A3', 'reported', { last_report: 'waiting', question: '用哪个分支？' }),
      member('A4', 'reported', { last_report: 'ready' }),
    ),
  );
  const open = vi.fn();
  render(<AgentTeamActivity sessionId="ses_main" onOpen={open} />);
  await act(async () => {});
  const part = screen.getByRole('region', { name: '子 Agent' });
  expect(part.textContent).toContain('2 个在运行');
  expect(part.textContent).toContain('共 4 个');
  expect(part.textContent).toContain('2 个要你处理');
  fireEvent.click(screen.getByRole('button', { name: '查看' }));
  expect(open).toHaveBeenCalledTimes(1);
});

test('none at work is said so', async () => {
  list.mockResolvedValue(team(member('A1', 'reported', { last_report: 'ready' })));
  render(<AgentTeamActivity sessionId="ses_main" onOpen={vi.fn()} />);
  await act(async () => {});
  const part = screen.getByRole('region', { name: '子 Agent' });
  expect(part.textContent).toContain('没有在运行的');
  expect(part.textContent).not.toContain('要你处理');
});

test('a conversation without sub-agents has no part for them', async () => {
  list.mockResolvedValue(team());
  const { container } = render(<AgentTeamActivity sessionId="ses_main" onOpen={vi.fn()} />);
  await act(async () => {});
  expect(container.textContent).toBe('');
});
