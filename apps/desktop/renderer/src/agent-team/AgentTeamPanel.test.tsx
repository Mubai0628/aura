import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { AgentTeamPanel } from './AgentTeamPanel';
import { resetPlacesForTests, writeTeamView } from './canvas/places';
import { focusedAgent, resetAgentFocusForTests } from './team-focus';
import { resetAgentTeamsForTests } from './team-store';

// The 多 Agent page: what needs the user first, then the main agent and its sub-agents with
// their model, state, tokens and current tool, on a canvas or in a list. Picking one shows its
// conversation in the main area; the page itself shows no record. The list is read here; the
// canvas has its own tests.

const list = vi.fn();
const stop = vi.fn();
const history = vi.fn();
const getApproval = vi.fn();
beforeEach(() => {
  writeTeamView('list');
  list.mockReset();
  stop.mockReset();
  history.mockReset().mockReturnValue(new Promise(() => {}));
  getApproval.mockReset().mockReturnValue(new Promise(() => {}));
  class QuietObserver {
    observe(): void {
      /* nothing is ever in view */
    }
    disconnect(): void {
      /* nothing to release */
    }
  }
  Object.defineProperty(globalThis, 'IntersectionObserver', {
    configurable: true,
    value: QuietObserver,
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listAgentTeam: list,
        stopAgentTeamMember: stop,
        readAgentHistory: history,
        getAgentApproval: getApproval,
        readAgentToolActivity: vi.fn().mockResolvedValue({ ok: true, value: { calls: [] } }),
      },
    },
  });
});
afterEach(() => {
  cleanup();
  resetAgentTeamsForTests();
  resetAgentFocusForTests();
  resetPlacesForTests();
  localStorage.clear();
});

const lead = (more: Record<string, unknown> = {}) => ({
  model_profile_id: 'mpr_sol',
  model: 'GPT 6.1 Sol',
  working: false,
  tokens: 0,
  ...more,
});

const member = (agent: string, name: string, more: Record<string, unknown> = {}) => ({
  agent,
  name,
  session_id: `ses_${agent}`,
  model_profile_id: 'mpr_flash',
  model: 'DeepSeek V4.1 Flash',
  state: 'running',
  working: true,
  approvals: [],
  tokens: 0,
  ...more,
});

const team = (members: unknown[], more: Record<string, unknown> = {}) => ({
  ok: true,
  value: { lead: lead(more), members },
});

async function shown(props: { expanded?: boolean; onToggleExpanded?: () => void } = {}) {
  render(<AgentTeamPanel sessionId="ses_main" visible {...props} />);
  await act(async () => {});
}

const rows = () =>
  within(screen.getByRole('list', { name: 'Agent 列表' }))
    .getAllByRole('listitem')
    .map((row) => row.textContent);

const rowButton = (name: string) =>
  within(screen.getByRole('list', { name: 'Agent 列表' })).getByRole('button', {
    name: new RegExp(name, 'u'),
  });

test('the main agent comes first, then each sub-agent with its model, state, tokens and tool', async () => {
  list.mockResolvedValue(
    team(
      [
        member('A1', '读代码', { tool: 'fs.read', tokens: 3456 }),
        member('A2', '查测试', {
          state: 'reported',
          working: false,
          last_report: 'ready',
          summary: '测试都在 tests/',
          tokens: 789,
        }),
      ],
      { working: true, tool: 'agents.wait', tokens: 12000 },
    ),
  );
  await shown();
  expect(list).toHaveBeenCalledWith({ session_id: 'ses_main' });
  const [main, first, second] = rows();
  for (const part of ['主 Agent', 'GPT 6.1 Sol', '运行中', '等子 Agent 报告', '12,000'])
    expect(main).toContain(part);
  for (const part of ['读代码', 'A1', 'DeepSeek V4.1 Flash', '运行中', '读取文件', '3,456'])
    expect(first).toContain(part);
  for (const part of ['查测试', 'A2', '已交报告', '789']) expect(second).toContain(part);
  expect(screen.queryByRole('region', { name: '要你处理' })).toBeNull();
});

test('what needs the user is listed first: approvals, questions, blocked and failed ones', async () => {
  list.mockResolvedValue(
    team([
      member('A1', '读代码', { approvals: ['apr_1'] }),
      member('A2', '定分支', {
        state: 'reported',
        working: false,
        last_report: 'waiting',
        summary: '要先定分支',
        question: '用哪个分支？',
      }),
      member('A3', '读配置', {
        state: 'reported',
        working: false,
        last_report: 'blocked',
        summary: '配置文件在工作区外',
      }),
      member('A4', '跑脚本', {
        state: 'reported',
        working: false,
        last_report: 'failed',
        summary: '这一轮出错停下了',
      }),
      member('A5', '已停的', { state: 'stopped', working: false, last_report: 'failed' }),
      member('A6', '交了的', { state: 'reported', working: false, last_report: 'ready' }),
    ]),
  );
  await shown();
  const asks = screen.getByRole('region', { name: '要你处理' });
  expect(getApproval).toHaveBeenCalledWith({ session_id: 'ses_A1', approval_id: 'apr_1' });
  expect(within(asks).getByRole('article', { name: '定分支 有问题' }).textContent).toContain(
    '用哪个分支？',
  );
  expect(within(asks).getByRole('article', { name: '读配置 卡住' }).textContent).toContain(
    '配置文件在工作区外',
  );
  expect(within(asks).getByRole('article', { name: '跑脚本 失败' }).textContent).toContain(
    '这一轮出错停下了',
  );
  expect(within(asks).queryByText(/已停的|交了的/u)).toBeNull();
  // 查看 shows that sub-agent in the main area.
  fireEvent.click(
    within(within(asks).getByRole('article', { name: '定分支 有问题' })).getByRole('button', {
      name: '查看',
    }),
  );
  expect(focusedAgent('ses_main')).toBe('A2');
  expect(rowButton('定分支').getAttribute('aria-current')).toBe('true');
  expect(screen.queryByRole('region', { name: /子 Agent 记录/u })).toBeNull();
});

test('a conversation without sub-agents lists its main agent alone', async () => {
  list.mockResolvedValue(team([]));
  await shown();
  expect(rows()).toHaveLength(1);
  expect(screen.getByText('没有子 Agent')).toBeTruthy();
  expect(screen.queryByRole('region', { name: /子 Agent 记录/u })).toBeNull();
  expect(history).not.toHaveBeenCalled();
});

test('picking a sub-agent shows it in the main area, and the main agent goes back', async () => {
  list.mockResolvedValue(team([member('A1', '读代码'), member('A2', '查测试')]));
  await shown();
  // Nothing is picked at first: the main area shows the conversation.
  expect(focusedAgent('ses_main')).toBeUndefined();
  expect(rowButton('主 Agent').getAttribute('aria-current')).toBe('true');
  fireEvent.click(rowButton('读代码'));
  expect(focusedAgent('ses_main')).toBe('A1');
  expect(rowButton('读代码').getAttribute('aria-current')).toBe('true');
  expect(rowButton('主 Agent').getAttribute('aria-current')).toBe('false');
  expect(screen.queryByRole('region', { name: /子 Agent 记录/u })).toBeNull();
  expect(history).not.toHaveBeenCalled();
  fireEvent.click(rowButton('主 Agent'));
  expect(focusedAgent('ses_main')).toBeUndefined();
  expect(rowButton('主 Agent').getAttribute('aria-current')).toBe('true');
});

test('the page can fill the main area and go back, and a pick there takes it back', async () => {
  list.mockResolvedValue(team([]));
  const toggle = vi.fn();
  await shown({ onToggleExpanded: toggle });
  fireEvent.click(screen.getByRole('button', { name: '放大多 Agent 页' }));
  expect(toggle).toHaveBeenCalledTimes(1);
  cleanup();
  await shown({ expanded: true, onToggleExpanded: toggle });
  expect(screen.getByRole('button', { name: '还原多 Agent 页' }).getAttribute('aria-pressed')).toBe(
    'true',
  );
  // The pick shows in the main area, which the page then leaves.
  cleanup();
  list.mockResolvedValue(team([member('A1', '读代码')]));
  await shown({ expanded: true, onToggleExpanded: toggle });
  toggle.mockClear();
  fireEvent.click(rowButton('读代码'));
  expect(focusedAgent('ses_main')).toBe('A1');
  expect(toggle).toHaveBeenCalledTimes(1);
});

test('the canvas shows until the list is chosen, and the choice is kept', async () => {
  localStorage.clear();
  list.mockResolvedValue(team([member('A1', '读代码')]));
  await shown();
  const canvas = screen.getByRole('group', { name: 'Agent 画布' });
  expect(within(canvas).getByRole('button', { name: /读代码/u })).toBeTruthy();
  expect(screen.getByRole('button', { name: '画布' }).getAttribute('aria-pressed')).toBe('true');
  expect(screen.queryByRole('list', { name: 'Agent 列表' })).toBeNull();
  // Picking a card shows that sub-agent in the main area, as a row does.
  fireEvent.click(within(canvas).getByRole('button', { name: /读代码/u }));
  expect(focusedAgent('ses_main')).toBe('A1');
  fireEvent.click(screen.getByRole('button', { name: '列表' }));
  expect(screen.getByRole('list', { name: 'Agent 列表' })).toBeTruthy();
  expect(screen.queryByRole('group', { name: 'Agent 画布' })).toBeNull();
  expect(rowButton('读代码').getAttribute('aria-current')).toBe('true');
  cleanup();
  await shown();
  expect(screen.getByRole('button', { name: '列表' }).getAttribute('aria-pressed')).toBe('true');
});
