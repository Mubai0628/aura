import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { AgentTeamPanel } from './AgentTeamPanel';
import { writeTeamView } from './canvas/places';
import { graphView, profile, team } from './graph-fixtures.test-support';
import { mergePositions } from './graph/graph-layout';
import { focusedAgent, resetAgentFocusForTests } from './team-focus';
import { resetAgentTeamsForTests } from './team-store';

const read = vi.fn();
const mutate = vi.fn();
const readProfile = vi.fn();
beforeEach(() => {
  writeTeamView('list');
  read.mockReset();
  mutate.mockReset();
  readProfile.mockReset();
  class QuietObserver {
    observe() {}
    disconnect() {}
  }
  Object.defineProperty(globalThis, 'IntersectionObserver', {
    configurable: true,
    value: QuietObserver,
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        readAgentGraph: read,
        mutateAgentGraph: mutate,
        readAgentProfile: readProfile,
        listAgentTeam: vi.fn().mockResolvedValue({ ok: true, value: team('ses_a') }),
        readAgentHistory: vi.fn().mockReturnValue(new Promise(() => {})),
        readAgentToolActivity: vi.fn().mockResolvedValue({ ok: true, value: { calls: [] } }),
      },
    },
  });
});
afterEach(() => {
  cleanup();
  resetAgentTeamsForTests();
  resetAgentFocusForTests();
  localStorage.clear();
});

test('both parent-local A1 records stay visible and picking the other one opens its real parent', async () => {
  read.mockResolvedValue({
    ok: true,
    value: graphView([
      ['ses_a', '规划'],
      ['ses_b', '开发'],
    ]),
  });
  const open = vi.fn().mockReturnValue(true);
  render(<AgentTeamPanel sessionId="ses_a" visible onOpenSession={open} />);
  const first = await screen.findByRole('region', { name: '规划的临时团队' });
  const second = screen.getByRole('region', { name: '开发的临时团队' });
  expect(within(first).getByText('A1')).toBeTruthy();
  expect(within(second).getByText('A1')).toBeTruthy();
  expect(read).toHaveBeenCalledWith({ session_id: 'ses_a' });
  const child = within(second).getByRole('button', { name: /检查\s*A1/u });
  fireEvent.click(child);
  expect(child.getAttribute('aria-pressed')).toBe('true');
  expect(open).not.toHaveBeenCalled();
  fireEvent.doubleClick(child);
  expect(open).toHaveBeenCalledWith('ses_b');
  expect(focusedAgent('ses_b')).toBe('A1');
  expect(focusedAgent('ses_a')).toBeUndefined();
  expect(mutate).not.toHaveBeenCalled();
});

test('the canvas offers a keyboard join without exposing the old graph forms', async () => {
  writeTeamView('canvas');
  const base = graphView([['ses_a', '规划']]);
  base.candidates = [profile('ses_b', '开发')];
  read.mockResolvedValue({ ok: true, value: base });
  render(<AgentTeamPanel sessionId="ses_a" visible />);
  await screen.findByRole('button', { name: '画布操作' });
  const board = screen.getByRole('group', { name: 'Agent 画布' });
  expect(screen.queryByRole('combobox', { name: '加入会话' })).toBeNull();
  fireEvent.contextMenu(board);
  expect(screen.queryByRole('menuitem', { name: '加入会话' })).toBeNull();
  fireEvent.keyDown(board, { key: 'Insert' });
  expect(within(board).getByRole('menuitem', { name: '开发' })).toBeTruthy();
  expect(mutate).not.toHaveBeenCalled();
});

test('archived sessions stay out of the board and its keyboard join choices', async () => {
  writeTeamView('canvas');
  const base = graphView([
    ['ses_a', '规划'],
    ['ses_b', '旧会话'],
  ]);
  base.graph.edges = [{ source_session_id: 'ses_a', target_session_id: 'ses_b' }];
  base.candidates = [profile('ses_c', '开发'), profile('ses_d', '旧候选会话')];
  read.mockResolvedValue({ ok: true, value: base });
  render(<AgentTeamPanel sessionId="ses_a" visible activeSessions={new Set(['ses_a', 'ses_c'])} />);
  await screen.findByRole('button', { name: '画布操作' });
  const board = screen.getByRole('group', { name: 'Agent 画布' });
  expect(within(board).queryByRole('button', { name: /^旧会话/u })).toBeNull();
  expect(board.querySelector('.agent-graph-peer-hit')).toBeNull();
  fireEvent.keyDown(board, { key: 'Insert' });
  expect(screen.getByRole('menuitem', { name: '开发' })).toBeTruthy();
  expect(screen.queryByRole('menuitem', { name: '旧候选会话' })).toBeNull();
});

test('editing from the list opens the selected parent profile without opening a different conversation', async () => {
  read.mockResolvedValue({
    ok: true,
    value: graphView([
      ['ses_a', '规划'],
      ['ses_b', '开发'],
    ]),
  });
  readProfile.mockResolvedValue({ ok: true, value: profile('ses_b', '开发') });
  const open = vi.fn();
  render(<AgentTeamPanel sessionId="ses_a" visible onOpenSession={open} />);
  const panel = await screen.findByRole('region', { name: '开发的临时团队' });
  expect(screen.queryByRole('textbox', { name: '会话职责简介' })).toBeNull();
  fireEvent.contextMenu(within(panel).getByRole('button', { name: /^主 Agent/u }));
  fireEvent.click(screen.getByRole('menuitem', { name: '编辑简介' }));
  const dialog = await screen.findByRole('dialog', { name: '编辑会话简介' });
  expect(within(dialog).getByRole('textbox', { name: '会话职责简介' })).toBeTruthy();
  expect(readProfile).toHaveBeenCalledWith({ session_id: 'ses_b' });
  expect(open).not.toHaveBeenCalled();
});

test('joining a member of another graph previews every affected member and binds both revisions', async () => {
  writeTeamView('canvas');
  const base = graphView([
    ['ses_a', '规划'],
    ['ses_b', '开发'],
  ]);
  base.candidates = [profile('ses_c', '测试'), profile('ses_d', '验收')];
  const other = graphView(
    [
      ['ses_c', '测试'],
      ['ses_d', '验收'],
    ],
    'acg_other',
    11,
  );
  read.mockImplementation(({ session_id }: { session_id: string }) =>
    Promise.resolve({ ok: true, value: session_id === 'ses_c' ? other : base }),
  );
  mutate.mockResolvedValue({
    ok: true,
    value: graphView(
      [
        ['ses_a', '规划'],
        ['ses_b', '开发'],
        ['ses_c', '测试'],
        ['ses_d', '验收'],
      ],
      'acg_one',
      8,
    ),
  });
  render(<AgentTeamPanel sessionId="ses_a" visible />);
  await screen.findByRole('button', { name: '画布操作' });
  fireEvent.keyDown(screen.getByRole('group', { name: 'Agent 画布' }), { key: 'Insert' });
  fireEvent.click(screen.getByRole('menuitem', { name: '测试' }));
  const dialog = await screen.findByRole('dialog', { name: '合并编排画布' });
  for (const name of ['规划', '开发', '测试', '验收'])
    expect(within(dialog).getByText(name)).toBeTruthy();
  expect(mutate).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole('button', { name: '合并画布' }));
  await waitFor(() =>
    expect(mutate).toHaveBeenCalledWith({
      session_id: 'ses_a',
      expected_graph: { graph_id: 'acg_one', revision: 7 },
      action: {
        action: 'merge',
        other_session_id: 'ses_c',
        expected_other: { graph_id: 'acg_other', revision: 11 },
        confirmed: true,
        positions: mergePositions(base, other),
      },
    }),
  );
});
