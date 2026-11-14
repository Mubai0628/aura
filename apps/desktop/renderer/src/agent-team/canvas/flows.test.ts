import type { AgentTeamMemberView, AgentTeamView } from '@aura/contracts/generated';
import { expect, test } from 'vitest';
import { exchanges } from './flows';

// What passed between the main agent and its sub-agents from one list of them to the next.

const member = (agent: string, more: Partial<AgentTeamMemberView> = {}) =>
  ({
    agent,
    name: agent,
    session_id: `ses_${agent}`,
    model_profile_id: 'mpr_flash',
    model: 'DeepSeek V4.1 Flash',
    state: 'running',
    working: true,
    approvals: [],
    tokens: 0,
    ...more,
  }) as AgentTeamMemberView;

const team = (...members: AgentTeamMemberView[]) =>
  ({
    lead: { model_profile_id: 'mpr_sol', model: 'GPT 6.1 Sol', working: true, tokens: 0 },
    members,
  }) as AgentTeamView;

test('the first list shows nothing passing; a new sub-agent gets its task sheet', () => {
  expect(exchanges(undefined, team(member('A1'))).size).toBe(0);
  expect(exchanges(team(), team(member('A1'))).get('A1')).toEqual({
    kind: 'task',
    up: false,
    label: '任务单',
  });
});

test('a new report goes up, by its kind; the same report again does not', () => {
  const working = team(member('A1'));
  const kinds = [
    ['ready', 'report', '报告'],
    ['progress', 'report', '进展'],
    ['waiting', 'question', '提问'],
    ['blocked', 'question', '受阻'],
    ['failed', 'failed', '没做成'],
  ] as const;
  for (const [status, kind, label] of kinds) {
    const after = team(member('A1', { state: 'reported', last_report: status, summary: status }));
    expect(exchanges(working, after).get('A1'), status).toEqual({ kind, up: true, label });
    expect(exchanges(after, after).size).toBe(0);
  }
  // A second progress report with a new summary is new.
  const one = team(member('A1', { last_report: 'progress', summary: '读完了一半' }));
  const two = team(member('A1', { last_report: 'progress', summary: '读完了' }));
  expect(exchanges(one, two).get('A1')?.label).toBe('进展');
});

test('a sub-agent that reported and works again was sent more', () => {
  const done = team(member('A1', { state: 'reported', working: false, last_report: 'ready' }));
  const again = team(member('A1', { state: 'running', last_report: 'ready' }));
  expect(exchanges(done, again).get('A1')).toEqual({ kind: 'task', up: false, label: '消息' });
  // Stopping with no new report passes nothing.
  const stopped = team(member('A1', { state: 'stopped', working: false, last_report: 'ready' }));
  expect(exchanges(again, stopped).size).toBe(0);
  // The report Aura gives the main agent about a stop goes up as the stop.
  const told = team(
    member('A1', { state: 'stopped', working: false, last_report: 'failed', summary: '停止了' }),
  );
  expect(exchanges(again, told).get('A1')).toEqual({ kind: 'failed', up: true, label: '已停止' });
});
