import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { CommandEntry, SessionModelSelectorProjection } from '@aura/contracts/generated';
import { ResizableComposer } from '../ResizableComposer';
import { resetComposerDraftsForTests, updateComposerDraft } from '../agent-session/composer-drafts';

const builtin = (name: string, title: string, kind: CommandEntry['kind'] = 'immediate') => ({
  name,
  title,
  description: `${title}说明`,
  kind,
  source: 'builtin' as const,
  argument_hint: kind === 'block' ? '补充说明（可选）' : null,
  template: null,
  enabled: true,
  issue: null,
  location: null,
  revision: null,
});
const catalog: CommandEntry[] = [
  builtin('init', '初始化工作区', 'block'),
  builtin('model', '切换模型'),
  builtin('permission', '切换权限'),
  builtin('stop', '停止当前任务'),
  builtin('compact', '压缩上下文', 'block'),
  builtin('context', '上下文'),
  builtin('tools', '工具设置'),
  builtin('memory', '记忆设置'),
  builtin('diff', '审查改动'),
  builtin('help', '命令帮助'),
  {
    ...builtin('review', '审查本次改动'),
    kind: 'prompt',
    source: 'workspace',
    template: '请审查 $ARGUMENTS 的改动，列出风险。',
  },
  { ...builtin('broken', '坏的命令'), kind: 'prompt', source: 'user', issue: '模板缺失' },
];
const selector = {
  binding_revision: '1',
  selected_model_profile_id: 'model',
  models: [],
  endpoints: [],
  providers: [],
} as unknown as SessionModelSelectorProjection;

const listCommands = vi.fn(),
  startInit = vi.fn(),
  submit = vi.fn(),
  compaction = vi.fn(),
  startCompaction = vi.fn(),
  stop = vi.fn(),
  openSettings = vi.fn(),
  openPanel = vi.fn(),
  openInspector = vi.fn();
beforeEach(() => {
  localStorage.clear();
  resetComposerDraftsForTests();
  for (const mock of [
    submit,
    startInit,
    compaction,
    startCompaction,
    stop,
    openSettings,
    openPanel,
    openInspector,
  ])
    mock.mockReset();
  startInit.mockResolvedValue({ ok: true, value: {} });
  listCommands.mockReset().mockResolvedValue({ ok: true, value: { commands: catalog } });
  submit.mockResolvedValue({ ok: true, value: {} });
  compaction.mockResolvedValue({ ok: true, value: {} });
  startCompaction.mockResolvedValue({ ok: true, value: {} });
  stop.mockResolvedValue(undefined);
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listCommands,
        submitAgentMessage: submit,
        requestAgentCompaction: compaction,
        startAgentCompaction: startCompaction,
        startWorkspaceInit: startInit,
      },
    },
  });
});
afterEach(cleanup);

function renderComposer(active = false) {
  render(
    <ResizableComposer
      sessionId="session"
      activeTurn={active ? 'turn' : undefined}
      activeRunId={active ? 'run' : undefined}
      workspaceId="workspace"
      queuePending={false}
      onChanged={vi.fn()}
      onStop={stop}
      demoMode={false}
      modelSelector={selector}
      onOpenModelSettings={vi.fn()}
      onOpenPromptInspector={openInspector}
      onOpenSecuritySettings={vi.fn()}
      onOpenSettings={openSettings}
      onOpenPanel={openPanel}
      onSelectModel={vi.fn()}
      onSetPermissionMode={vi.fn()}
      permissionMode="workspace_read_write"
      promptInspector={undefined}
    />,
  );
  return screen.getByRole('textbox', { name: '输入消息' }) as HTMLTextAreaElement;
}
const menu = () => screen.queryByRole('listbox', { name: '命令' });

test('a typed slash at the start, a line start or after a space opens the menu', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '/');
  expect(await screen.findByRole('listbox', { name: '命令' })).toBeTruthy();
  expect(within(menu()!).getByText('/model')).toBeTruthy();
  expect(within(menu()!).queryByText('/broken')).toBeNull();
  await user.keyboard('{Escape}');
  expect(menu()).toBeNull();
  expect(box.value).toBe('/');
  await user.clear(box);
  await user.type(box, 'a/');
  expect(menu()).toBeNull();
  await user.type(box, ' /');
  expect(menu()).toBeTruthy();
  await user.type(box, '{Enter}');
  await user.clear(box);
  await user.type(box, '第一行{Shift>}{Enter}{/Shift}/');
  expect(menu()).toBeTruthy();
});

test('pasted slash text never opens the menu and is sent exactly as written', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  box.focus();
  await user.paste('/plan 先做计划');
  expect(menu()).toBeNull();
  await user.keyboard('{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith({
      session_id: 'session',
      text: '/plan 先做计划',
      destination: { lane: 'next_turn' },
    }),
  );
});

test('a restored draft never opens the menu and is sent exactly as written', async () => {
  updateComposerDraft('session', { text: '/model 恢复的草稿', needsReview: false });
  const user = userEvent.setup();
  const box = renderComposer();
  expect(box.value).toBe('/model 恢复的草稿');
  await user.click(box);
  await user.keyboard('{End}');
  expect(menu()).toBeNull();
  await user.keyboard('{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith(expect.objectContaining({ text: '/model 恢复的草稿' })),
  );
});

test('Enter while an input method is composing neither picks a command nor sends', async () => {
  const user = userEvent.setup();
  const box = renderComposer(true);
  await user.type(box, '/too');
  fireEvent.keyDown(box, { key: 'Enter', isComposing: true });
  expect(openSettings).not.toHaveBeenCalled();
  expect(menu()).toBeTruthy();
  expect(submit).not.toHaveBeenCalled();
  expect(box.value).toBe('/too');
});

test('an unmatched slash word is ordinary text and Enter sends it', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '/xyz');
  expect(screen.getByRole('status').textContent).toContain('没有匹配的命令');
  await user.keyboard('{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith(expect.objectContaining({ text: '/xyz' })),
  );
});

test('keyboard picks run immediate commands and leave the rest of the draft', async () => {
  const user = userEvent.setup();
  const box = renderComposer(true);
  await user.type(box, '保留 /too');
  await user.keyboard('{Enter}');
  expect(openSettings).toHaveBeenCalledWith('tools');
  expect(box.value).toBe('保留 ');
  await user.type(box, '/di{Tab}');
  expect(openPanel).toHaveBeenCalledWith('review');
  await user.type(box, '/sto{Enter}');
  expect(stop).toHaveBeenCalledOnce();
  await user.type(box, '/con{ArrowDown}{ArrowUp}{Enter}');
  expect(openInspector).toHaveBeenCalledOnce();
  expect(submit).not.toHaveBeenCalled();
});

test('a command that cannot run now is shown with its reason and cannot be picked', async () => {
  const user = userEvent.setup();
  const box = renderComposer(false);
  await user.type(box, '/stop');
  const option = within(menu()!).getByRole('option', { name: /停止当前任务/u });
  expect(option.getAttribute('aria-disabled')).toBe('true');
  expect(option.textContent).toContain('只在任务运行中可用');
  await user.keyboard('{Enter}');
  expect(stop).not.toHaveBeenCalled();
  expect(box.value).toBe('/stop');
});

test('/compact becomes a block, and sending registers compaction with its focus', async () => {
  const user = userEvent.setup();
  const box = renderComposer(true);
  await user.type(box, '/comp{Enter}');
  expect(screen.getByRole('group', { name: '命令块' }).textContent).toContain('/compact');
  expect(box.value).toBe('');
  await user.type(box, '保留测试结论{Enter}');
  await waitFor(() =>
    expect(compaction).toHaveBeenCalledWith({
      session_id: 'session',
      agent_run_id: 'run',
      focus: '保留测试结论',
    }),
  );
  expect(submit).not.toHaveBeenCalled();
  expect(screen.queryByRole('group', { name: '命令块' })).toBeNull();
  expect(screen.getByRole('status').textContent).toContain('已登记压缩');
});

// With nothing running, /compact starts a
// task of its own that compacts first.
test('/compact can be used while nothing runs: it starts a compaction task with its focus', async () => {
  const user = userEvent.setup();
  const box = renderComposer(false);
  await user.type(box, '/comp');
  const option = within(menu()!).getByRole('option', { name: /压缩上下文/u });
  expect(option.getAttribute('aria-disabled')).not.toBe('true');
  await user.keyboard('{Enter}');
  await user.type(box, '保留测试结论{Enter}');
  await waitFor(() =>
    expect(startCompaction).toHaveBeenCalledWith({
      session_id: 'session',
      model_profile_id: expect.any(String),
      focus: '保留测试结论',
    }),
  );
  expect(compaction).not.toHaveBeenCalled();
  expect(submit).not.toHaveBeenCalled();
  expect(screen.queryByRole('group', { name: '命令块' })).toBeNull();
});

test('a block is removed with its button or Backspace at the start', async () => {
  const user = userEvent.setup();
  const box = renderComposer(true);
  await user.type(box, '/comp{Enter}');
  await user.keyboard('{Backspace}');
  expect(screen.queryByRole('group', { name: '命令块' })).toBeNull();
  await user.type(box, '/comp{Enter}');
  await user.click(screen.getByRole('button', { name: '移除命令 /compact' }));
  expect(screen.queryByRole('group', { name: '命令块' })).toBeNull();
});

test('a prompt command inserts its template as ordinary editable text', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '/rev{Enter}');
  expect(box.value).toBe('请审查  的改动，列出风险。');
  // Keep typing where the caret went (user.type would click and move it to the end).
  await user.keyboard('登录模块');
  expect(box.value).toBe('请审查 登录模块 的改动，列出风险。');
});

test('each opening rereads the catalog; a failed reread keeps the last list and says so', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await waitFor(() => expect(listCommands).toHaveBeenCalledTimes(1));
  const fresh = {
    ...builtin('fresh', '新写的命令'),
    kind: 'prompt',
    source: 'user',
    template: '新模板',
  };
  listCommands.mockResolvedValue({ ok: true, value: { commands: [...catalog, fresh] } });
  await user.type(box, '/');
  expect(await within(menu()!).findByText('/fresh')).toBeTruthy();
  await user.keyboard('{Escape}');
  listCommands.mockResolvedValue({ ok: false, error: { code: 'x', message: 'x' } });
  await user.clear(box);
  await user.type(box, '/');
  await waitFor(() => expect(listCommands).toHaveBeenCalledTimes(3));
  expect(await screen.findByText(/命令列表读取失败/u)).toBeTruthy();
  expect(within(menu()!).getByText('/fresh')).toBeTruthy();
});

test('the plus menu and /help list every offered command', async () => {
  const user = userEvent.setup();
  const box = renderComposer(true);
  await user.click(screen.getByRole('button', { name: '添加' }));
  await user.click(screen.getByRole('menuitem', { name: /命令/u }));
  expect(within(menu()!).getAllByRole('option').length).toBe(11);
  await user.keyboard('{Escape}');
  await user.type(box, '/hel{Enter}');
  expect(menu()).toBeTruthy();
  expect(listCommands).toHaveBeenCalledWith({ workspace_id: 'workspace' });
});

// A press outside closes the list, not only a pick or deleting the `/`.
test('a press outside the list closes it, also when the plus menu opened it', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '/');
  expect(menu()).toBeTruthy();
  await user.click(document.body);
  expect(menu()).toBeNull();
  expect(box.value).toBe('/');
  await user.click(screen.getByRole('button', { name: '添加' }));
  await user.click(screen.getByRole('menuitem', { name: /命令/u }));
  expect(menu()).toBeTruthy();
  await user.click(document.body);
  expect(menu()).toBeNull();
});

// The chosen line stays in view.
test('the line chosen with the arrow keys is scrolled into view', async () => {
  const scrolled: Element[] = [];
  Object.defineProperty(Element.prototype, 'scrollIntoView', {
    configurable: true,
    value(this: Element) {
      scrolled.push(this);
    },
  });
  try {
    const user = userEvent.setup();
    const box = renderComposer();
    await user.type(box, '/');
    await user.keyboard('{ArrowDown}{ArrowDown}{ArrowDown}{ArrowDown}');
    const chosen = within(menu()!)
      .getAllByRole('option')
      .find((option) => option.getAttribute('aria-selected') === 'true');
    expect(chosen?.textContent).toContain('/compact');
    expect(scrolled.at(-1)).toBe(chosen);
  } finally {
    Reflect.deleteProperty(Element.prototype, 'scrollIntoView');
  }
});

test('/init becomes a block; sending starts the init task with the note, never a message', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '/ini{Enter}');
  expect(screen.getByRole('group', { name: '命令块' }).textContent).toContain('/init');
  expect(box.placeholder).toBe('补充说明（可选）');
  await user.type(box, '重点写测试命令{Enter}');
  await waitFor(() =>
    expect(startInit).toHaveBeenCalledWith({
      session_id: 'session',
      model_profile_id: 'model',
      text: '重点写测试命令',
    }),
  );
  expect(submit).not.toHaveBeenCalled();
  await waitFor(() => expect(screen.queryByRole('group', { name: '命令块' })).toBeNull());
  expect(box.value).toBe('');
});

test('/init waits for an idle session and says why', async () => {
  const user = userEvent.setup();
  const box = renderComposer(true);
  await user.type(box, '/init');
  const option = within(menu()!).getByRole('option', { name: /初始化工作区/u });
  expect(option.getAttribute('aria-disabled')).toBe('true');
  expect(option.textContent).toContain('当前任务结束、排队消息处理完之后才能开始');
  await user.keyboard('{Enter}');
  expect(startInit).not.toHaveBeenCalled();
});
