import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ReviewPanel } from './ReviewPanel';
import { resetReviewOptionsForTests } from './review-options';
import { resetReviewStoreForTests } from './review-store';
import { installGit, session, workspace } from './review-fixture';

// Every control the review page shows works — the file list and its
// grouping, tests and generated files apart, side by side, wrapping, changed words, collapsing
// and expanding every file, filling the main area and closing.

beforeEach(() => {
  resetReviewOptionsForTests();
  resetReviewStoreForTests();
  installGit();
});
afterEach(cleanup);

const card = (path: string) => screen.getByRole('article', { name: `${path} 的改动` });
const option = async (user: ReturnType<typeof userEvent.setup>, name: string) => {
  await user.click(screen.getByRole('button', { name: '更多审查选项' }));
  await user.click(screen.getByRole('menuitemcheckbox', { name }));
  await user.keyboard('{Escape}');
};

async function shown() {
  const user = userEvent.setup();
  const expand = vi.fn();
  const close = vi.fn();
  render(
    <ReviewPanel
      workspaceId={workspace}
      sessionId={session}
      onToggleExpanded={expand}
      onClose={close}
    />,
  );
  await screen.findByText('5 个文件 · +17 −11');
  await waitFor(() => expect(card('src/app.ts').querySelectorAll('.diff-row')).toHaveLength(5));
  return { user, expand, close };
}

test('the file list hides and shows; picked files are grouped by folder when asked', async () => {
  const { user } = await shown();
  await user.click(screen.getByRole('button', { name: '隐藏文件列表' }));
  expect(screen.queryByRole('navigation', { name: '改动的文件' })).toBeNull();
  await user.click(screen.getByRole('button', { name: '显示文件列表' }));
  await option(user, '按文件夹分组');
  const list = screen.getByRole('navigation', { name: '改动的文件' });
  expect([...list.querySelectorAll('.review-list-folder > p')].map((p) => p.textContent)).toEqual([
    'notes',
    '根目录',
    'src',
    'tests',
  ]);
  expect(within(list).getByRole('button', { name: /app\.ts/u }).title).toBe('src/app.ts');
});

test('tests and generated files can sit apart from the source', async () => {
  const { user } = await shown();
  await option(user, '测试与生成文件分开');
  expect(
    [...document.querySelectorAll('.review-cards-group')].map((heading) => heading.textContent),
  ).toEqual(['源码与其他', '测试文件', '构建与生成文件']);
  const list = screen.getByRole('navigation', { name: '改动的文件' });
  expect(
    [...list.querySelectorAll('.review-list-group')].map(
      (heading) => heading.firstChild?.textContent,
    ),
  ).toEqual(['源码与其他', '测试文件', '构建与生成文件']);
});

test('side by side puts the removed line beside the one that replaced it', async () => {
  const { user } = await shown();
  await option(user, '左右对照');
  const table = card('src/app.ts').querySelector('table')!;
  expect(table.dataset.mode).toBe('split');
  const pair = [...table.querySelectorAll('.diff-row')][1]!;
  expect([...pair.querySelectorAll('td')].map((cell) => cell.textContent)).toEqual([
    '2',
    'console.log("hello");',
    '2',
    'console.log("hello, world");',
  ]);
});

test('changed words are marked unless turned off; long lines wrap when asked', async () => {
  const { user } = await shown();
  expect(
    [...card('src/app.ts').querySelectorAll('.diff-word')].map((word) => word.textContent),
  ).toEqual([', world']);
  await option(user, '高亮改动的词');
  expect(card('src/app.ts').querySelectorAll('.diff-word')).toHaveLength(0);
  expect(card('src/app.ts').querySelector('table')!.dataset.wrap).toBe('false');
  await option(user, '自动换行');
  expect(card('src/app.ts').querySelector('table')!.dataset.wrap).toBe('true');
});

test('every file collapses and expands at once, and one at a time', async () => {
  const { user } = await shown();
  await user.click(screen.getByRole('button', { name: '更多审查选项' }));
  await user.click(screen.getByRole('menuitem', { name: '全部收起' }));
  expect(
    [...document.querySelectorAll('.review-card')].every(
      (item) => item.getAttribute('data-collapsed') === 'true',
    ),
  ).toBe(true);
  expect(card('src/app.ts').querySelector('table')).toBeNull();
  await user.click(within(card('src/app.ts')).getByRole('button', { name: /src\/app\.ts/u }));
  expect(card('src/app.ts').querySelector('table')).not.toBeNull();
  await user.click(screen.getByRole('button', { name: '更多审查选项' }));
  await user.click(screen.getByRole('menuitem', { name: '全部展开' }));
  expect(document.querySelectorAll('.review-card[data-collapsed="true"]')).toHaveLength(0);
});

test('picking a file in the list opens its changes', async () => {
  const { user } = await shown();
  await user.click(within(card('README.md')).getByRole('button', { name: /README\.md/u }));
  expect(card('README.md').dataset.collapsed).toBe('true');
  const list = screen.getByRole('navigation', { name: '改动的文件' });
  await user.click(within(list).getByRole('button', { name: /README\.md/u }));
  expect(card('README.md').dataset.collapsed).toBe('false');
});

test('the page fills the main area, and closes, from its own buttons', async () => {
  const { user, expand, close } = await shown();
  await user.click(screen.getByRole('button', { name: '放大审查页' }));
  expect(expand).toHaveBeenCalledTimes(1);
  await user.click(screen.getByRole('button', { name: '关闭审查页' }));
  expect(close).toHaveBeenCalledTimes(1);
});
