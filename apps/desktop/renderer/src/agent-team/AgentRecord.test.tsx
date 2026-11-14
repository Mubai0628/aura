import type { AgentTeamMemberView } from '@aura/contracts/generated';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { AgentRecord } from './AgentRecord';
import { focusAgent, focusedAgent, resetAgentFocusForTests } from './team-focus';
import { resetAgentTeamsForTests } from './team-store';

// A sub-agent picked on the 多 Agent page, in the main area: the way back to the conversation,
// its state, and its whole record read only, with its approvals and 停止.

const list = vi.fn();
const stop = vi.fn();
const history = vi.fn();
const getApproval = vi.fn();
beforeEach(() => {
  list.mockReset().mockReturnValue(new Promise(() => {}));
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
});

const member = (more: Partial<AgentTeamMemberView> = {}): AgentTeamMemberView =>
  ({
    agent: 'A1',
    name: '读代码',
    session_id: 'ses_A1',
    model_profile_id: 'mpr_flash',
    model: 'DeepSeek V4.1 Flash',
    state: 'running',
    working: true,
    approvals: [],
    tokens: 0,
    ...more,
  }) as AgentTeamMemberView;

async function shown(of: AgentTeamMemberView) {
  focusAgent('ses_main', of.agent);
  render(<AgentRecord parentSessionId="ses_main" parentTitle="改多 Agent 页" member={of} />);
  await act(async () => {});
  return screen.getByRole('region', { name: `子 Agent 记录：${of.name}` });
}

test('its record is read only, its path goes back, and 停止 stops it', async () => {
  const sheet = '来自主 Agent 的任务单：读代码';
  history.mockResolvedValue({
    ok: true,
    value: {
      session_id: 'ses_A1',
      runtime_instance_id: 'runtime',
      revision: '4',
      active: null,
      turns: [
        {
          turn: {
            mode: 'normal',
            state: { state: 'recovery_required', reason: 'agent.core_restarted' },
            session_id: 'ses_A1',
            turn_task_id: 'trn_1',
            revision: '3',
            root_user_event_id: 'evt_sheet',
          },
          user_messages: [
            {
              event_id: 'evt_sheet',
              text: sheet,
              total_bytes: new TextEncoder().encode(sheet).length,
            },
          ],
          assistant_messages: [],
          plan: null,
          todo: { items: [] },
        },
      ],
    },
  });
  const stopped = {
    ok: true,
    value: {
      lead: { model_profile_id: 'mpr_sol', model: 'GPT 6.1 Sol', working: false, tokens: 0 },
      members: [member({ state: 'stopped', working: false })],
    },
  };
  stop.mockResolvedValue(stopped);
  const record = await shown(member());
  await waitFor(() => expect(record.textContent).toContain(sheet));
  expect(history).toHaveBeenCalledWith(expect.objectContaining({ session_id: 'ses_A1' }));
  expect(within(record).getByText('运行中')).toBeTruthy();
  // Read only: no 继续 for its cut-off turn, and nothing to type into.
  expect(within(record).queryByRole('button', { name: '继续' })).toBeNull();
  expect(within(record).queryByRole('textbox')).toBeNull();
  fireEvent.click(within(record).getByRole('button', { name: '停止' }));
  await waitFor(() => expect(stop).toHaveBeenCalledWith({ session_id: 'ses_main', agent: 'A1' }));
  // The conversation's title leads back to it.
  fireEvent.click(within(record).getByRole('button', { name: '改多 Agent 页' }));
  expect(focusedAgent('ses_main')).toBeUndefined();
});

test('a stopped sub-agent has no 停止, and a failed stop says so and keeps it', async () => {
  const ended = await shown(member({ state: 'stopped', working: false }));
  expect(within(ended).queryByRole('button', { name: '停止' })).toBeNull();
  expect(within(ended).getByText('已停止')).toBeTruthy();
  cleanup();
  stop.mockResolvedValue({
    ok: false,
    error: { code: 'agents.unknown_agent', message: '没有子 Agent A1。' },
  });
  const record = await shown(member());
  fireEvent.click(within(record).getByRole('button', { name: '停止' }));
  expect(await within(record).findByRole('alert')).toBeTruthy();
  expect(within(record).getByRole('button', { name: '停止' })).toBeTruthy();
});

test('the approvals it waits for are decided where it is shown', async () => {
  const record = await shown(member({ approvals: ['apr_1'] }));
  expect(getApproval).toHaveBeenCalledWith({ session_id: 'ses_A1', approval_id: 'apr_1' });
  expect(within(record).getByRole('region', { name: '当前任务审批' })).toBeTruthy();
});

test('its record has no mode button: its card on the 多 Agent page switches the mode', async () => {
  const record = await shown(member({ mode: 'read_only' }));
  const head = record.querySelector<HTMLElement>('.agent-team-record-head');
  expect(head).not.toBeNull();
  expect(head?.querySelector('[aria-haspopup="menu"]')).toBeNull();
  expect(
    within(head as HTMLElement).queryByRole('button', { name: /只读|工作区读写|完全访问|权限/ }),
  ).toBeNull();
});

test('what the main agent and the settings answered for it is shown with why', async () => {
  const record = await shown(
    member({
      state: 'running',
      reviewing: ['apr_02'],
      reviews: [
        {
          approval_id: 'apr_01',
          answered: { by: 'main_agent', answer: 'allow', reason: '只删它的旧截图', at: 'x' },
        },
        {
          approval_id: 'apr_00',
          answered: {
            by: 'settings',
            answer: 'allow',
            reason: '设置里允许子 Agent 下载。',
            at: 'x',
          },
        },
        { approval_id: 'apr_02' },
      ],
    } as Partial<AgentTeamMemberView>),
  );
  expect(within(record).getByText('主 Agent 审核中')).toBeTruthy();
  const answers = within(record).getByRole('list', { name: '代批记录' });
  expect(answers.textContent).toContain('主 Agent 代批：允许 · 只删它的旧截图');
  expect(answers.textContent).toContain('设置：允许 · 设置里允许子 Agent 下载。');
});

test('a workspace allowance the main agent gave is named as the card names it', async () => {
  const record = await shown(
    member({
      reviews: [
        {
          approval_id: 'apr_03',
          answered: {
            by: 'main_agent',
            answer: 'allow_workspace',
            reason: '删它为检查建的临时文件',
            at: 'x',
          },
        },
      ],
    } as Partial<AgentTeamMemberView>),
  );
  const answers = within(record).getByRole('list', { name: '代批记录' });
  expect(answers.textContent).toContain('主 Agent 代批：本工作区允许 · 删它为检查建的临时文件');
});
