import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { TerminalViewV1 } from '@aura/contracts/generated';
import { RightWorkspace } from '../RightWorkspace';
import { resetToolTabsForTests } from './tool-tabs';
import { resetTerminalStoreForTests } from '../terminal/store';
import { resetUiSettingsForTests } from '../ui-settings-store';

// The right sidebar holds tabs. 审查 and the other single tools open once and
// then leave "+"; every 终端 is a new tab that opens the default terminal chosen in 设置 → 环境,
// with no shell to pick. Closing a terminal you opened ends it; the model's terminal becomes a tab
// only when you ask to see it, and closing that tab leaves it running.

vi.mock('../terminal/xterm-runtime', () => ({
  terminalEngine: () => ({ terminal: { focus: vi.fn(), options: {} } }),
  mountTerminal: () => () => {},
  disposeTerminalEngine: vi.fn(),
}));
vi.mock('../review/ReviewPanel', () => ({ ReviewPanel: () => <p>审查内容</p> }));
vi.mock('../terminal/ProcessPanel', () => ({
  ProcessPanel: ({ onViewTerminal }: { onViewTerminal: (id: string) => void }) => (
    <button type="button" onClick={() => onViewTerminal('trm_model')}>
      查看终端
    </button>
  ),
}));

const session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const view = (terminal_id: string, owner: 'user' | 'agent', title: string): TerminalViewV1 =>
  ({
    terminal_id,
    session_id: session,
    owner,
    controller: owner,
    shell: 'cmd',
    title,
    revision: '1',
    process_id: 1,
    started_at: '2026-10-19T00:00:00Z',
    end: null,
  }) as TerminalViewV1;

let opened = 0;
const api = {
  list: vi.fn(),
  beginOpen: vi.fn(),
  cancelOpen: vi.fn(),
  onOpening: vi.fn(() => () => {}),
  onState: vi.fn(),
  onOutput: vi.fn(),
  subscribe: vi.fn(),
  close: vi.fn(),
};
beforeEach(() => {
  resetToolTabsForTests();
  resetTerminalStoreForTests();
  resetUiSettingsForTests();
  opened = 0;
  api.list.mockReset().mockResolvedValue({
    ok: true,
    value: { terminals: [view('trm_model', 'agent', 'Agent CMD')], next_cursor: null },
  });
  api.beginOpen
    .mockReset()
    .mockImplementation(() =>
      Promise.resolve({ ok: true, value: view(`trm_own_${++opened}`, 'user', 'CMD') }),
    );
  api.subscribe.mockReset().mockResolvedValue({
    ok: true,
    value: {
      session_id: session,
      terminal_id: 'x',
      runtime_instance_id: 'run_1',
      start_offset: 0,
      next_offset: 0,
      data_base64: '',
      truncated: false,
      end: null,
    },
  });
  api.close.mockReset().mockResolvedValue({ ok: true, value: {} });
  vi.stubGlobal('aura', {
    v1: {
      terminal: api,
      onCoreHealth: vi.fn(() => () => {}),
      getUiSettings: vi.fn().mockResolvedValue({
        ok: true,
        value: {
          revision: 2,
          settings: { show_debug_information: false, default_terminal: 'cmd' },
          issue: null,
        },
      }),
    },
  });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function renderSidebar() {
  render(
    <RightWorkspace
      visible
      workspaceId="wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV"
      sessionId={session}
      onToggleSidebar={vi.fn()}
      promptInspector={undefined}
    />,
  );
}

const menuItems = () =>
  within(screen.getByRole('menu', { name: '选择工具' }))
    .getAllByRole('menuitem')
    .map((item) => item.textContent?.replace(/Ctrl.*$/u, '').trim());
const tabs = () =>
  within(screen.getByRole('tablist', { name: '已打开的工具' }))
    .getAllByRole('tab')
    .map((tab) => tab.textContent?.trim());

test('审查 opens once and leaves "+"; each 终端 is a new tab', async () => {
  const user = userEvent.setup();
  renderSidebar();
  await user.click(screen.getByRole('button', { name: '打开工具' }));
  // 提示词监测 opens from the context meter, `/context` and a step, not "+".
  expect(menuItems()).toEqual(['审查', '终端', '进程', '浏览器', '文件', '侧边对话']);
  await user.click(screen.getByRole('menuitem', { name: /审查/u }));
  expect(await screen.findByText('审查内容')).toBeTruthy();
  await user.click(screen.getByRole('button', { name: '打开工具' }));
  expect(menuItems()).not.toContain('审查');
  await user.click(screen.getByRole('menuitem', { name: /终端/u }));
  await user.click(screen.getByRole('button', { name: '打开工具' }));
  await user.click(screen.getByRole('menuitem', { name: /终端/u }));
  expect(tabs()).toEqual(['审查', '终端 1', '终端 2']);
});

test('a terminal tab opens the default terminal from 设置 → 环境; there is no shell to pick', async () => {
  const user = userEvent.setup();
  renderSidebar();
  await user.click(screen.getByRole('button', { name: '打开工具' }));
  await user.click(screen.getByRole('menuitem', { name: /终端/u }));
  await vi.waitFor(() => expect(api.beginOpen).toHaveBeenCalledTimes(1));
  expect(api.beginOpen.mock.calls[0]![0]).toMatchObject({
    request: { session_id: session, shell: 'cmd' },
  });
  expect(screen.queryByRole('combobox')).toBeNull();
  expect(screen.queryByRole('button', { name: '新建终端' })).toBeNull();
});

test('closing a terminal you opened ends it; the model’s terminal tab only hides it', async () => {
  const user = userEvent.setup();
  renderSidebar();
  await user.click(screen.getByRole('button', { name: '打开工具' }));
  await user.click(screen.getByRole('menuitem', { name: /终端/u }));
  await vi.waitFor(() =>
    expect(api.subscribe).toHaveBeenCalledWith(
      expect.objectContaining({ terminal_id: 'trm_own_1' }),
    ),
  );
  await user.click(screen.getByRole('button', { name: '关闭终端 1' }));
  expect(api.close).toHaveBeenCalledWith({ session_id: session, terminal_id: 'trm_own_1' });
  // The model's terminal: listed, but no tab until the process page asks to show it.
  expect(screen.queryByRole('tab', { name: /Agent CMD/u })).toBeNull();
  await user.click(screen.getByRole('button', { name: '打开工具' }));
  await user.click(screen.getByRole('menuitem', { name: /进程/u }));
  await user.click(screen.getByRole('button', { name: '查看终端' }));
  const model = await screen.findByRole('tab', { name: /Agent CMD/u });
  expect(model.getAttribute('aria-selected')).toBe('true');
  await act(async () => {
    await user.click(screen.getByRole('button', { name: '关闭Agent CMD' }));
  });
  expect(screen.queryByRole('tab', { name: /Agent CMD/u })).toBeNull();
  expect(api.close).toHaveBeenCalledTimes(1);
});

// A tab's right-click menu closes several at once; terminals you opened end
// with their tabs, so closing them that way asks first and says how many.
test('closing the other tabs, terminals you opened among them, asks first', async () => {
  const user = userEvent.setup();
  renderSidebar();
  for (const tool of [/审查/u, /终端/u, /终端/u]) {
    await user.click(screen.getByRole('button', { name: '打开工具' }));
    await user.click(screen.getByRole('menuitem', { name: tool }));
  }
  await vi.waitFor(() =>
    expect(api.subscribe).toHaveBeenCalledWith(
      expect.objectContaining({ terminal_id: 'trm_own_2' }),
    ),
  );
  fireEvent.contextMenu(screen.getByRole('tab', { name: /审查/u }));
  await user.click(screen.getByRole('menuitem', { name: '关闭其他标签' }));
  const dialog = await screen.findByRole('dialog', { name: '关闭标签并结束终端？' });
  expect(dialog.textContent).toContain('2 个你打开的终端');
  expect(api.close).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole('button', { name: '关闭并结束终端' }));
  await vi.waitFor(() => expect(api.close).toHaveBeenCalledTimes(2));
  expect(tabs()).toEqual(['审查']);
});
