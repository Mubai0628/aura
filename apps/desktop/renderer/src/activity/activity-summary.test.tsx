import type { AgentModeEpisodeView } from '@aura/contracts/generated';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { createRef } from 'react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ActivityNoteStack } from '../ActivityNoteStack';
import { installGit, session, workspace } from '../review/review-fixture';
import { resetReviewStoreForTests } from '../review/review-store';

// "会话区右上角的“近期活动”卡片展示的内容还有按钮、布局的位置，以及再丰富其上展示
// 的内容如总结审查当前未提交内容改变的行数汇总". The card says what the conversation is doing, what is
// uncommitted (lines included) with a way to the review, and what was done last.

const episode = {
  session_id: session,
  revision: '3',
  mode: 'normal',
  paused: false,
  goal_budget: null,
  inherited_goal: null,
} as unknown as AgentModeEpisodeView;

const call = (
  id: string,
  title: string,
  target: string | null,
  at: string,
  outcome = 'succeeded',
) => ({
  model_step_id: 'step',
  display: { title, target, summary: '', details: [], truncated: false, internal: false },
  summary: {
    tool_call_id: id,
    status: 'finalized',
    outcome: { type: outcome },
    started_at: at,
    completed_at: at,
  },
});

let activity: ReturnType<typeof vi.fn>;
beforeEach(() => {
  resetReviewStoreForTests();
  const git = installGit();
  // The calls' times are fixed before the card reads them.
  const now = Date.now();
  const calls = [
    call('tcl_3', '写入文件', 'maze-game.html', new Date(now - 30_000).toISOString()),
    call('tcl_2', '搜索文本', null, new Date(now - 5 * 60_000 - 1_000).toISOString(), 'failed'),
    call('tcl_1', '读取文件', 'AGENTS.md', new Date(now - 2 * 3_600_000 - 1_000).toISOString()),
  ];
  activity = vi.fn(async () => ({
    ok: true,
    value: {
      session_id: session,
      next_before_call_id: null,
      calls,
    },
  }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { git: window.aura.v1.git ?? git, readAgentToolActivity: activity } },
  });
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function show(
  props: Partial<Parameters<typeof ActivityNoteStack>[0]['summary'] & object> = {},
  slot?: HTMLElement,
) {
  const onOpenReview = vi.fn();
  render(
    <div>
      <div className="conversation-column" />
      <ActivityNoteStack
        combinedSidebarWidthRatio={0.2}
        conversationColumnRef={createRef()}
        viewportRef={createRef()}
        episode={episode}
        entrySlot={slot}
        summary={{
          workspaceId: workspace,
          sessionId: session,
          running: false,
          lastEndedAt: new Date(Date.now() - 3 * 60_000).toISOString(),
          refreshKey: 'turn:1',
          onOpenReview,
          ...props,
        }}
      />
    </div>,
  );
  return { onOpenReview };
}

test('the card says what the conversation is doing, what is uncommitted and what was done last', async () => {
  const { onOpenReview } = show();
  const card = screen.getByRole('complementary', { name: '近期活动' });
  expect(within(card).getByText('空闲 · 上次完成于 3 分钟前')).toBeTruthy();
  expect(within(card).getByText('Normal')).toBeTruthy();
  // Three parts,
  // and the review's own totals, new files' lines included, added lines green and deleted red.
  await waitFor(() =>
    expect(
      within(card)
        .getAllByRole('region')
        .map((part) => part.getAttribute('aria-label')),
    ).toEqual(['会话', '未提交改动', '最近操作']),
  );
  const changes = within(card).getByRole('region', { name: '未提交改动' });
  await waitFor(() => expect(changes.textContent).toContain('5 个文件 · +17 −11'));
  expect(within(changes).getByText('+17').className).toBe('activity-added');
  expect(within(changes).getByText('−11').className).toBe('activity-deleted');
  // The mode belongs with the conversation's state.
  expect(
    within(within(card).getByRole('region', { name: '会话' })).getByText('Normal'),
  ).toBeTruthy();
  await userEvent.setup().click(within(card).getByRole('button', { name: '查看审查' }));
  expect(onOpenReview).toHaveBeenCalledOnce();
  const recent = await within(card).findByRole('list', { name: '最近操作' });
  expect(
    within(recent)
      .getAllByRole('listitem')
      .map((item) => item.textContent),
  ).toEqual([
    '写入文件maze-game.html刚刚',
    '搜索文本执行失败5 分钟前',
    '读取文件AGENTS.md2 小时前',
  ]);
  expect(within(recent).getAllByRole('listitem')[1]!.dataset.failed).toBe('true');
  expect(activity).toHaveBeenCalledWith({ session_id: session, before_call_id: null, limit: 3 });
});

test('a running conversation says so', () => {
  show({ running: true });
  expect(screen.getByText('正在运行')).toBeTruthy();
});

test('a conversation waiting for the user says 要你处理, as its sidebar row does', () => {
  show({ running: true, waiting: true });
  const state = within(screen.getByRole('region', { name: '会话' })).getByText('要你处理');
  expect(state.dataset.waiting).toBe('true');
  expect(screen.queryByText('正在运行')).toBeNull();
});

test('with nothing uncommitted the card says so, and a folder outside Git says that instead', async () => {
  const empty = vi.fn(async () => ({
    ok: true,
    value: { state: 'unavailable', executable: null, version: null, message: '', next_step: '' },
  }));
  window.aura.v1.git.runtime = empty as never;
  show();
  expect(await screen.findByText('未检测到 Git，无法汇总改动')).toBeTruthy();
});

// A plain folder read as 「读不到这个工作区的 Git 状态」, and a Git older than 2.40 as 「未检测到 Git」.
test('a plain folder, an old Git and a failed read each say what they are', async () => {
  window.aura.v1.git.read = vi.fn(async () => ({
    ok: false,
    error: { code: 'git.not_repository', message: 'git.not_repository: 此工作区不是 Git 仓库。' },
  })) as never;
  show();
  expect(await screen.findByText('这个工作区不是 Git 仓库，没有可汇总的改动')).toBeTruthy();
  cleanup();

  resetReviewStoreForTests();
  window.aura.v1.git.read = vi.fn(async () => ({
    ok: false,
    error: { code: 'git.read_failed', message: 'git.read_failed: Git 读取没有成功。' },
  })) as never;
  show();
  expect(await screen.findByText('暂时读不到 Git 状态，稍后会再读')).toBeTruthy();
  cleanup();

  resetReviewStoreForTests();
  window.aura.v1.git.runtime = vi.fn(async () => ({
    ok: true,
    value: {
      state: 'too_old',
      executable: 'git.exe',
      version: '2.30.0',
      message: '',
      next_step: '',
    },
  })) as never;
  show();
  expect(await screen.findByText('本机 Git 版本低于 2.40，无法汇总改动')).toBeTruthy();
});

test('in a narrow window the entry sits where it is given: the conversation header', () => {
  const slot = document.createElement('div');
  document.body.append(slot);
  const conversation = document.createElement('div');
  render(
    <ActivityNoteStack
      combinedSidebarWidthRatio={0.5}
      conversationColumnRef={{ current: conversation }}
      viewportRef={{ current: conversation }}
      episode={episode}
      entrySlot={slot}
    />,
  );
  expect(within(slot).getByRole('button', { name: /展开近期活动/u })).toBeTruthy();
  slot.remove();
});

test('in a narrow window, 查看审查 opens the review and puts the card away', async () => {
  const slot = document.createElement('div');
  document.body.append(slot);
  const conversation = document.createElement('div');
  const onOpenReview = vi.fn();
  render(
    <ActivityNoteStack
      combinedSidebarWidthRatio={0.5}
      conversationColumnRef={{ current: conversation }}
      viewportRef={{ current: conversation }}
      episode={episode}
      entrySlot={slot}
      summary={{
        workspaceId: workspace,
        sessionId: session,
        running: false,
        refreshKey: 'turn:1',
        onOpenReview,
      }}
    />,
  );
  const user = userEvent.setup();
  await user.click(within(slot).getByRole('button', { name: /展开近期活动/u }));
  const card = screen.getByRole('complementary', { name: '近期活动' });
  await user.click(await within(card).findByRole('button', { name: '查看审查' }));
  expect(onOpenReview).toHaveBeenCalledOnce();
  expect(screen.queryByRole('complementary', { name: '近期活动' })).toBeNull();
  expect(within(slot).getByRole('button', { name: /展开近期活动/u })).toBeTruthy();
  slot.remove();
});
