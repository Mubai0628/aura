import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { GitWorkspaceReadRequestV1 } from '@aura/contracts/generated';
import { RightWorkspace } from '../RightWorkspace';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';
import { ReviewPanel } from './ReviewPanel';
import { resetReviewOptionsForTests } from './review-options';
import { resetReviewStoreForTests } from './review-store';
import {
  answer,
  diff,
  installGit,
  listings,
  patches,
  session,
  stat,
  status,
  workspace,
} from './review-fixture';

// The review page after the owner's reference. It compares the working
// tree with HEAD (未提交的改动), with where HEAD left the default branch (全部改动) or with another
// branch; lists the changed files with their counts; reads each file's diff on its own.

vi.mock('../terminal/TerminalHost', () => ({ TerminalHost: () => null }));
vi.mock('../terminal/ProcessPanel', () => ({ ProcessPanel: () => null }));

let git: ReturnType<typeof installGit>;
beforeEach(() => {
  resetToolTabsForTests();
  resetReviewOptionsForTests();
  resetReviewStoreForTests();
  git = installGit();
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const panel = () => <ReviewPanel workspaceId={workspace} sessionId={session} />;
const card = (path: string) => screen.getByRole('article', { name: `${path} 的改动` });
const rows = (path: string) =>
  [...card(path).querySelectorAll('.diff-row')].map((row) =>
    [...row.querySelectorAll('td')].map((cell) => cell.textContent),
  );

test('the uncommitted changes show with their counts, the new file among them, each file read on its own', async () => {
  render(panel());
  expect(await screen.findByText('5 个文件 · +17 −11')).toBeTruthy();
  expect(screen.getByRole('button', { name: /未提交的改动/u })).toBeTruthy();
  expect(listings(git.read)).toEqual([
    // The list is the files and their line counts alone.
    {
      base: null,
      paths: [],
      ignore_whitespace: false,
      stats_only: true,
      offset_bytes: 0,
      max_bytes: 1,
    },
  ]);
  const list = screen.getByRole('navigation', { name: '改动的文件' });
  expect(
    within(list)
      .getAllByRole('button')
      .map((row) => row.title),
  ).toEqual(['notes/new.md', 'pnpm-lock.yaml', 'README.md', 'src/app.ts', 'tests/app.test.ts']);
  await waitFor(() => expect(rows('src/app.ts')).toHaveLength(5));
  expect(rows('src/app.ts')).toEqual([
    ['1', '1', ' const name = "aura";'],
    ['2', '', '−console.log("hello");'],
    ['', '2', '+console.log("hello, world");'],
    ['', '3', '+export {};'],
    ['3', '4', ' run();'],
  ]);
  const own = git.read.mock.calls.find(
    ([request]) =>
      request.query.kind === 'review_diff' && request.query.input.paths?.[0] === 'src/app.ts',
  )?.[0];
  expect(own).toEqual({
    workspace_id: workspace,
    session_id: session,
    query: {
      kind: 'review_diff',
      input: {
        base: null,
        paths: ['src/app.ts'],
        ignore_whitespace: false,
        offset_bytes: 0,
        max_bytes: 65_536,
      },
    },
  });
  await waitFor(() => expect(card('notes/new.md').textContent).toContain('新的笔记'));
  expect(within(card('notes/new.md')).getByText('新文件')).toBeTruthy();
  expect(
    git.read.mock.calls.some(
      ([request]) =>
        request.query.kind === 'untracked_diff' && request.query.input.path === 'notes/new.md',
    ),
  ).toBe(true);
  expect(screen.queryByRole('button', { name: '提交' })).toBeNull();
});

test('全部改动 compares with the default branch, and another branch can be picked', async () => {
  const user = userEvent.setup();
  render(panel());
  await screen.findByText('5 个文件 · +17 −11');
  await user.click(screen.getByRole('button', { name: /未提交的改动/u }));
  const menu = screen.getByRole('menu', { name: '比较范围' });
  expect(
    within(menu)
      .getAllByRole('menuitemradio')
      .map((item) => item.textContent),
  ).toEqual(['全部改动（与 main 比较）', '未提交的改动', 'feature', 'origin/main']);
  await user.click(within(menu).getByRole('menuitemradio', { name: '全部改动（与 main 比较）' }));
  await waitFor(() => expect(listings(git.read).at(-1)?.base).toBe('main'));
  expect(screen.getByRole('button', { name: /全部改动 · 与 main 比较/u })).toBeTruthy();
  await user.click(screen.getByRole('button', { name: /全部改动 · 与 main 比较/u }));
  await user.click(screen.getByRole('menuitemradio', { name: 'origin/main' }));
  await waitFor(() => expect(listings(git.read).at(-1)?.base).toBe('origin/main'));
  await waitFor(() =>
    expect(
      git.read.mock.calls.some(
        ([request]) =>
          request.query.kind === 'review_diff' &&
          request.query.input.base === 'origin/main' &&
          request.query.input.paths?.[0] === 'src/app.ts',
      ),
    ).toBe(true),
  );
});

test('hiding whitespace changes asks Git to ignore them', async () => {
  const user = userEvent.setup();
  render(panel());
  await screen.findByText('5 个文件 · +17 −11');
  await user.click(screen.getByRole('button', { name: '更多审查选项' }));
  await user.click(screen.getByRole('menuitemcheckbox', { name: '隐藏空白改动' }));
  await waitFor(() => expect(listings(git.read).at(-1)?.ignore_whitespace).toBe(true));
});

test('branch, upstream, operation and conflicts show, and a file note stays with its file', async () => {
  git.read.mockImplementation(async (request: GitWorkspaceReadRequestV1) => {
    if (request.query.kind === 'status')
      return {
        ok: true,
        value: {
          kind: 'status',
          result: { ...status(), operation: 'rebase', conflicts: ['src/app.ts'] },
        },
      };
    if (request.query.kind === 'review_diff' && !request.query.input.paths?.length)
      return {
        ok: true,
        value: {
          kind: 'diff',
          result: diff('', [
            { ...stat('src/app.ts', 2, 1), comparison_note: '未运行外部程序，按原始内容比较' },
          ]),
        },
      };
    return answer(request);
  });
  render(panel());
  expect(
    await screen.findByText(/上游 origin\/feature · 领先 2 · 落后 1 · 进行中：变基 · 冲突 1/u),
  ).toBeTruthy();
  expect(within(card('src/app.ts')).getByText('未运行外部程序，按原始内容比较')).toBeTruthy();
});

test('a folder outside Git says why, and the page can be read again', async () => {
  const user = userEvent.setup();
  git.read.mockResolvedValue({
    ok: false,
    error: { code: 'git.not_repository', message: '所选目录不是 Git 仓库' },
  } as never);
  render(panel());
  expect((await screen.findByRole('alert')).textContent).toContain('核对工作区位置和 Git 配置');
  git.read.mockImplementation(async (request: GitWorkspaceReadRequestV1) => answer(request));
  await user.click(screen.getByRole('button', { name: '更多审查选项' }));
  await user.click(screen.getByRole('menuitem', { name: '刷新' }));
  expect(await screen.findByText('5 个文件 · +17 −11')).toBeTruthy();
});

test('a long diff shows a part, and the rest on request', async () => {
  const user = userEvent.setup();
  // Longer than the half megabyte a file shows before asking for more.
  const line = `+${'x'.repeat(999)}\n`;
  const long = `diff --git a/big.txt b/big.txt\n@@ -1,1 +1,540 @@\n${line.repeat(540)}`;
  git.read.mockImplementation(async (request: GitWorkspaceReadRequestV1) => {
    const query = request.query;
    if (query.kind === 'review_diff' && !query.input.paths?.length)
      return { ok: true, value: { kind: 'diff', result: diff('', [stat('big.txt', 540, 0)]) } };
    if (query.kind === 'review_diff') {
      const start = query.input.offset_bytes ?? 0;
      const end = Math.min(long.length, start + 65_536);
      return {
        ok: true,
        value: {
          kind: 'diff',
          result: diff(long.slice(start, end), [], start, end < long.length ? end : null),
        },
      };
    }
    return answer(request);
  });
  render(panel());
  const more = await screen.findByRole('button', { name: '继续读取' });
  const shown = card('big.txt').querySelectorAll('.diff-row').length;
  expect(shown).toBeGreaterThan(500);
  expect(shown).toBeLessThan(540);
  await user.click(more);
  await waitFor(() => expect(card('big.txt').querySelectorAll('.diff-row').length).toBe(540));
  expect(screen.queryByRole('button', { name: '继续读取' })).toBeNull();
});

test('a binary file shows no text diff', async () => {
  git.read.mockImplementation(async (request: GitWorkspaceReadRequestV1) =>
    request.query.kind === 'review_diff' && !request.query.input.paths?.length
      ? {
          ok: true,
          value: { kind: 'diff', result: diff('', [{ ...stat('logo.png', 0, 0), binary: true }]) },
        }
      : answer(request),
  );
  render(panel());
  await waitFor(() => expect(card('logo.png').textContent).toContain('二进制文件，不显示差异。'));
  expect(card('logo.png').querySelector('table')).toBeNull();
});

test.each(['missing', 'too_old'] as const)(
  'unavailable Git (%s) says why and how to install it, without reading the repository',
  async (state) => {
    git.runtime.mockResolvedValue({
      ok: true,
      value: {
        state,
        message: state === 'missing' ? '未找到 Git' : 'Git 版本低于 2.40',
        next_step: '请安装或升级本机 Git。',
        installation_url: 'https://git-scm.com/downloads/win',
      },
    } as never);
    render(panel());
    expect(await screen.findByText('Git 不可用')).toBeTruthy();
    expect(screen.getByText('请安装或升级本机 Git。')).toBeTruthy();
    expect(git.read).not.toHaveBeenCalled();
    git.openInstallation.mockResolvedValue({
      ok: false,
      error: { message: '安装页面未打开' },
    } as never);
    fireEvent.click(screen.getByRole('button', { name: '打开 Git 安装页面' }));
    expect((await screen.findByRole('alert')).textContent).toContain('git-scm.com');
  },
);

// The review in the right sidebar: read only while shown, per conversation.
const sidebar = {
  workspaceId: workspace,
  sessionId: session,
  onToggleSidebar: vi.fn(),
  promptInspector: undefined,
};

test('a closed sidebar does not read; showing it reads the conversation’s repository', async () => {
  toolTabs.open(session, 'review');
  const shown = render(<RightWorkspace {...sidebar} visible={false} />);
  expect(git.runtime).not.toHaveBeenCalled();
  shown.rerender(<RightWorkspace {...sidebar} visible />);
  expect(await screen.findByText('5 个文件 · +17 −11')).toBeTruthy();
});

test('late data from the conversation left cannot enter the next one’s review', async () => {
  toolTabs.open(session, 'review');
  toolTabs.open('session-two', 'review');
  let finish: ((value: unknown) => void) | undefined;
  git.read.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }) as never,
  );
  const shown = render(<RightWorkspace {...sidebar} />);
  await waitFor(() => expect(finish).toBeDefined());
  shown.rerender(
    <RightWorkspace {...sidebar} workspaceId="workspace-two" sessionId="session-two" />,
  );
  await screen.findByText('5 个文件 · +17 −11');
  await act(async () =>
    finish?.({
      ok: true,
      value: { kind: 'status', result: { ...status(), branch: 'old-private-branch' } },
    }),
  );
  expect(screen.queryByText(/old-private-branch/u)).toBeNull();
});

test('tool completions read the page again once per burst', async () => {
  const shown = render(
    <ReviewPanel workspaceId={workspace} sessionId={session} completionKey="a" />,
  );
  await screen.findByText('5 个文件 · +17 −11');
  git.read.mockClear();
  vi.useFakeTimers();
  shown.rerender(<ReviewPanel workspaceId={workspace} sessionId={session} completionKey="b" />);
  shown.rerender(<ReviewPanel workspaceId={workspace} sessionId={session} completionKey="c" />);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(300);
  });
  expect(git.read.mock.calls.filter(([request]) => request.query.kind === 'status')).toHaveLength(
    1,
  );
});

test('thousands of files keep one page of cards, a bounded list, and at most two reads at once', async () => {
  const many = Array.from({ length: 2500 }, (_, index) =>
    stat(`f/${String(index).padStart(4, '0')}.txt`, 1, 0),
  );
  const pending: ((value: unknown) => void)[] = [];
  git.read.mockImplementation(async (request: GitWorkspaceReadRequestV1) => {
    const query = request.query;
    if (query.kind === 'review_diff' && !query.input.paths?.length)
      return { ok: true, value: { kind: 'diff', result: diff('', many) } };
    if (query.kind === 'review_diff')
      return new Promise((resolve) => pending.push(resolve)) as never;
    return answer(request);
  });
  render(panel());
  await screen.findByText('2501 个文件 · +2500 −0');
  expect(document.querySelectorAll('.review-card')).toHaveLength(100);
  expect(document.querySelectorAll('.review-list-row')).toHaveLength(1000);
  await waitFor(() => expect(pending).toHaveLength(2));
  await act(async () => {
    pending[0]!({ ok: true, value: { kind: 'diff', result: diff(patches['README.md']!, []) } });
  });
  await waitFor(() => expect(pending).toHaveLength(3));
  fireEvent.click(screen.getByRole('button', { name: '显示更多文件（还有 2401 个）' }));
  expect(document.querySelectorAll('.review-card')).toHaveLength(200);
  // Rendering 2,500 rows took 17.7 s on a hosted CI runner, past the suite's 15 s default.
}, 60_000);
