import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ACTIVITY_EVERY_MS } from '../session-activity';
import { resetAgentTeamsForTests, useAgentTeam, useAgentTeamUnread } from './team-store';

// Every view of a conversation's sub-agents shares one read loop: it reads again while a view
// follows the conversation or a sub-agent works, and once otherwise.

const list = vi.fn();
beforeEach(() => {
  vi.useFakeTimers();
  list.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { listAgentTeam: list } },
  });
});
afterEach(() => {
  cleanup();
  resetAgentTeamsForTests();
  vi.useRealTimers();
});

const lead = { working: false, tokens: 0 };
const member = (agent: string, state: string) => ({
  agent,
  name: agent,
  session_id: `ses_${agent}`,
  model_profile_id: 'mpr_flash',
  model: 'Flash',
  state,
  working: state === 'running',
  approvals: [],
  tokens: 0,
});
const team = (...members: unknown[]) => ({ ok: true, value: { lead, members } });

function View({ session, follow }: { session: string; follow: boolean }) {
  const view = useAgentTeam(session, follow);
  return (
    <p>
      {session}:{view ? view.members.map((m) => `${m.agent} ${m.state}`).join(',') : 'unread'}
    </p>
  );
}

test('the views of one conversation share one read loop', async () => {
  list.mockResolvedValue(team(member('A1', 'running')));
  render(
    <>
      <View session="ses_main" follow />
      <View session="ses_main" follow={false} />
      <View session="ses_main" follow />
    </>,
  );
  await act(async () => {});
  expect(list).toHaveBeenCalledTimes(1);
  expect(screen.getAllByText('ses_main:A1 running')).toHaveLength(3);
  await act(async () => vi.advanceTimersByTime(ACTIVITY_EVERY_MS));
  expect(list).toHaveBeenCalledTimes(2);
});

test('with nothing to follow and nothing at work it reads once', async () => {
  list.mockResolvedValue(team(member('A1', 'reported')));
  render(<View session="ses_main" follow={false} />);
  await act(async () => {});
  await act(async () => vi.advanceTimersByTime(ACTIVITY_EVERY_MS * 4));
  expect(list).toHaveBeenCalledTimes(1);
  expect(screen.getByText('ses_main:A1 reported')).toBeTruthy();
});

test('a sub-agent at work keeps it reading until it has reported', async () => {
  list
    .mockResolvedValueOnce(team(member('A1', 'running')))
    .mockResolvedValue(team(member('A1', 'reported')));
  render(<View session="ses_main" follow={false} />);
  await act(async () => {});
  await act(async () => vi.advanceTimersByTime(ACTIVITY_EVERY_MS));
  expect(list).toHaveBeenCalledTimes(2);
  await act(async () => vi.advanceTimersByTime(ACTIVITY_EVERY_MS * 4));
  expect(list).toHaveBeenCalledTimes(2);
  expect(screen.getByText('ses_main:A1 reported')).toBeTruthy();
});

test('each conversation has its own list', async () => {
  list.mockImplementation(async ({ session_id }: { session_id: string }) =>
    session_id === 'ses_one' ? team(member('A1', 'reported')) : team(),
  );
  render(
    <>
      <View session="ses_one" follow={false} />
      <View session="ses_two" follow={false} />
    </>,
  );
  await act(async () => {});
  expect(screen.getByText('ses_one:A1 reported')).toBeTruthy();
  expect(screen.getByText('ses_two:')).toBeTruthy();
});

// A list that could not be read says so where it is shown, and is read again by itself.
test('a list that could not be read is marked and read again', async () => {
  list
    .mockResolvedValueOnce({ ok: false, error: { code: 'ipc.request_timeout', message: 'x' } })
    .mockResolvedValue(team(member('A1', 'reported')));
  function Unread() {
    return <p>{useAgentTeamUnread('ses_main') ? 'unread now' : 'read'}</p>;
  }
  render(
    <>
      <View session="ses_main" follow={false} />
      <Unread />
    </>,
  );
  await act(async () => {});
  expect(screen.getByText('unread now')).toBeTruthy();
  await act(async () => vi.advanceTimersByTime(3_000));
  expect(list).toHaveBeenCalledTimes(2);
  expect(screen.getByText('read')).toBeTruthy();
  expect(screen.getByText('ses_main:A1 reported')).toBeTruthy();
});
