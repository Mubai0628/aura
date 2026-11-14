import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type {
  SessionModelSelectorProjection,
  SkillView,
  TerminalViewV1,
} from '@aura/contracts/generated';
import type { TerminalApiV1 } from '../../../shared/terminal-api';
import { ResizableComposer } from '../ResizableComposer';
import { resetComposerDraftsForTests } from '../agent-session/composer-drafts';
import { browserStore } from '../browser/browser-store';
import { browserTabs } from '../browser/browser-tabs';
import type { FileActionScope } from '../files/file-menu';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';
import { addTerminal, configureTerminalText, resetTerminalStoreForTests } from '../terminal/store';
import { bufferText } from '../terminal/buffer-text';

const skill = (name: string, pinned = false, state: SkillView['state'] = 'ready'): SkillView => ({
  id: `skl_${name}`,
  source: 'agent_global',
  workspace_id: null,
  relative: name,
  path: `C:\\Users\\me\\.agents\\skills\\${name}`,
  state,
  enabled: true,
  name,
  description: `${name} 的说明`,
  license: null,
  compatibility: null,
  allowed_tools: [],
  fingerprint: `${name}-fingerprint`,
  trusted_fingerprint: `${name}-fingerprint`,
  warnings: [],
  missing_references: [],
  problems: [],
  file_count: 1,
  bytes: 10,
  pinned,
  pinned_left_out: false,
});
const selector = {
  binding_revision: '1',
  selected_model_profile_id: 'model',
  models: [],
  endpoints: [],
  providers: [],
} as unknown as SessionModelSelectorProjection;
const terminal = (id: string, owner: 'user' | 'agent', title = ''): TerminalViewV1 => ({
  session_id: 'session',
  terminal_id: id,
  shell: 'power_shell',
  process_id: 1,
  started_at: '2026-11-01T00:00:00Z',
  end: null,
  owner,
  controller: owner,
  title,
  tool_call_id: null,
  revision: '1',
});

const submit = vi.fn(),
  command = vi.fn(),
  resources = vi.fn(),
  handOver = vi.fn();
beforeEach(async () => {
  localStorage.clear();
  resetComposerDraftsForTests();
  resetTerminalStoreForTests();
  resetToolTabsForTests();
  browserTabs.resetForTests();
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  handOver.mockReset().mockImplementation(async (request: { terminal_id: string }) => ({
    ok: true,
    value: {
      ...terminal(request.terminal_id, request.terminal_id === 't1' ? 'user' : 'agent'),
      controller: 'agent',
      revision: '2',
      ...(request.terminal_id === 't1' ? { handed_over: true } : {}),
    },
  }));
  resources.mockReset().mockResolvedValue({
    ok: true,
    value: {
      resources: [
        {
          server_id: 'notes',
          server_name: '笔记',
          uri: 'fixture://notes/readme',
          name: 'readme',
          title: 'Fixture notes',
          mime_type: 'text/markdown',
        },
      ],
      problems: [{ server_id: 'docs', server_name: '文档', reason: '服务器没有启动' }],
    },
  });
  command.mockReset().mockImplementation(async (request: { kind: string; pageId: string }) =>
    request.kind === 'text' && (request.pageId === 'page-1' || request.pageId === 'model')
      ? {
          ok: true,
          text: {
            title: 'Example',
            url: 'https://example.com/a',
            text: 'Hello from the page',
            truncated: false,
          },
        }
      : request.kind === 'hand-over' && request.pageId === 'page-1'
        ? { ok: true, state: { ...browserStore.get('session', 'page-1'), handedAs: 'p1' } }
        : { ok: true },
  );
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listCommands: vi.fn().mockResolvedValue({ ok: true, value: { commands: [] } }),
        findWorkspacePaths: vi.fn().mockResolvedValue({
          ok: true,
          value: { catalog_revision: '1', matches: [] },
        }),
        listSkills: vi.fn().mockResolvedValue({
          ok: true,
          value: {
            revision: 1,
            skills: [skill('review'), skill('notes', true), skill('draft', false, 'not_trusted')],
            roots: [],
            issue: null,
          },
        }),
        submitAgentMessage: submit,
        listMcpResources: resources,
        browser: { command, pages: vi.fn().mockResolvedValue([]), onState: () => () => {} },
        terminal: { handOver },
      },
    },
  });
  const api = {
    subscribe: vi.fn().mockResolvedValue({
      ok: true,
      value: {
        runtime_instance_id: 'rt',
        session_id: 'session',
        terminal_id: 't1',
        start_offset: 0,
        next_offset: 0,
        data_base64: '',
        truncated: false,
        end: null,
      },
    }),
  } as unknown as TerminalApiV1;
  await addTerminal(terminal('t1', 'user'), api);
  await addTerminal(terminal('t2', 'agent', 'cargo test'), api);
  toolTabs.adoptTerminal('session', 't1');
  configureTerminalText(async (id, lines) =>
    lines !== 2000
      ? undefined
      : id === 't1'
        ? 'PS C:\\w> cargo test\nerror[E0425]'
        : id === 't2'
          ? 'test result: ok'
          : undefined,
  );
  browserTabs.add('session', 'page-1');
  browserStore.put({
    sessionId: 'session',
    pageId: 'page-1',
    url: 'https://example.com/a',
    title: 'Example',
    loading: false,
    canGoBack: false,
    canGoForward: false,
    agentActive: false,
    userInControl: false,
    closed: false,
  });
});
afterEach(cleanup);

function renderComposer() {
  const scope = { session: 'session', workspace: 'workspace', root: 'C:\\w', notify: vi.fn() };
  render(
    <ResizableComposer
      sessionId="session"
      workspaceId="workspace"
      queuePending={false}
      onChanged={vi.fn()}
      onStop={vi.fn()}
      demoMode={false}
      modelSelector={selector}
      onOpenModelSettings={vi.fn()}
      onOpenPromptInspector={vi.fn()}
      onOpenSecuritySettings={vi.fn()}
      onSelectModel={vi.fn()}
      onSetPermissionMode={vi.fn()}
      permissionMode="workspace_read_write"
      promptInspector={undefined}
      fileScope={scope as unknown as FileActionScope}
    />,
  );
  return screen.getByRole('textbox', { name: '输入消息' }) as HTMLTextAreaElement;
}
const group = (name: string) => screen.queryByRole('listbox', { name });
const options = (name: string) =>
  within(group(name)!)
    .getAllByRole('option')
    .map((option) => option.textContent);

test('"@" lists the usable Skills; a pinned one is noted as loaded, and one picked is the Skill chosen for the turn', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '@');
  await waitFor(() => expect(group('Skill')).toBeTruthy());
  expect(options('Skill')).toEqual(['review', 'notes已加载']);
  const pinned = within(group('Skill')!).getByRole('option', { name: /notes/ });
  expect(pinned.getAttribute('aria-disabled')).toBe('true');
  await user.keyboard('rev');
  await waitFor(() => expect(options('Skill')).toEqual(['review']));
  await user.keyboard('{Enter}');
  // The typed "@rev" went; the Skill is chosen as with "+ → Skill".
  expect(box.value).toBe('');
  expect(
    within(screen.getByRole('group', { name: '本轮使用的 Skill' })).getByText('Skill：review'),
  ).toBeTruthy();
  await user.type(box, '看看{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith({
      session_id: 'session',
      text: '看看',
      destination: { lane: 'next_turn' },
      skill_ids: ['skl_review'],
    }),
  );
});

test('"@" leaves out a terminal that has ended, unless its tab is still open', async () => {
  const ended = {
    reason: 'core_stopped',
    exit_code: null,
    stop_confirmed: false,
    error: null,
  } as const;
  const api = {
    subscribe: vi.fn(async (request: { terminal_id: string }) => ({
      ok: true,
      value: {
        runtime_instance_id: 'rt',
        session_id: 'session',
        terminal_id: request.terminal_id,
        start_offset: 0,
        next_offset: 0,
        data_base64: '',
        truncated: false,
        end: ended,
      },
    })),
  } as unknown as TerminalApiV1;
  // The model's terminal from an earlier run of Aura, and the user's that ended in its tab.
  await addTerminal({ ...terminal('t3', 'agent', 'npm run dev'), end: ended }, api);
  await addTerminal({ ...terminal('t4', 'user'), end: ended }, api);
  toolTabs.adoptTerminal('session', 't4');
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '@');
  await waitFor(() => expect(group('终端')).toBeTruthy());
  expect(options('终端')).toEqual(['终端 1', 'cargo test', '终端 2']);
});

test('a terminal picked from "@" is handed to the model and sent as what it shows, its last 2,000 lines', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '@');
  await waitFor(() => expect(group('终端')).toBeTruthy());
  expect(options('终端')).toEqual(['终端 1', 'cargo test']);
  await user.keyboard('zd');
  await waitFor(() => expect(options('终端')).toEqual(['终端 1']));
  await user.keyboard('{Enter}');
  expect(box.value).toBe('');
  expect(within(screen.getByRole('group', { name: '引用' })).getByText('终端 1')).toBeTruthy();
  await user.type(box, '这个报错怎么修{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith({
      session_id: 'session',
      text: '这个报错怎么修',
      destination: { lane: 'next_turn' },
      attachments: [
        {
          kind: 'capture',
          name: '终端 1.txt',
          text: 'Terminal "终端 1" (PowerShell 7; the user handed it to you as terminal `t1`: terminal.read reads what it prints from now on and terminal.write types into it; it stays the user\'s, who can take it back at any time, and terminal.close is refused) as it was when this message was sent, up to its last 2,000 lines:\n\nPS C:\\w> cargo test\nerror[E0425]\n',
        },
      ],
    }),
  );
  // Handed over before the message goes, so the model's first step finds it its own.
  expect(handOver).toHaveBeenCalledWith({ session_id: 'session', terminal_id: 't1' });
  expect(handOver.mock.invocationCallOrder[0]).toBeLessThan(submit.mock.invocationCallOrder[0]!);
  await waitFor(() => expect(screen.queryByRole('group', { name: '引用' })).toBeNull());
});

test('a terminal that cannot be handed over keeps the message, saying why', async () => {
  handOver.mockResolvedValue({
    ok: false,
    error: { code: 'terminal.ended', message: '终端已结束，请新建终端。' },
  });
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '@zd');
  await waitFor(() => expect(options('终端')).toEqual(['终端 1']));
  await user.keyboard('{Enter}');
  await user.type(box, '看看{Enter}');
  await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('终端已结束'));
  expect(screen.getByRole('alert').textContent).toContain(
    '「终端 1」没能交给模型：终端已结束，请新建终端。消息没有发出，内容仍在输入框里。请移除这个引用或稍后再发。',
  );
  expect(submit).not.toHaveBeenCalled();
  expect(box.value).toBe('看看');
});

test("a terminal the model opened is given back to it and sent with its id, for the model's terminal tools", async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '@cargo');
  await waitFor(() => expect(group('终端')).toBeTruthy());
  await user.keyboard('{Enter}');
  await user.type(box, '看结果{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith(
      expect.objectContaining({
        attachments: [
          {
            kind: 'capture',
            name: 'cargo test.txt',
            text: 'Terminal "cargo test" (PowerShell 7; your terminal `t2`, which terminal.read reads further) as it was when this message was sent, up to its last 2,000 lines:\n\ntest result: ok\n',
          },
        ],
      }),
    ),
  );
  expect(handOver).toHaveBeenCalledWith({ session_id: 'session', terminal_id: 't2' });
});

test('a page picked from "@" is handed to the model and sent with its title, address, text and page id; one that cannot be handed over or read is not sent', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '@');
  await waitFor(() => expect(group('浏览器标签页')).toBeTruthy());
  expect(options('浏览器标签页')).toEqual(['Examplehttps://example.com/a']);
  await user.click(within(group('浏览器标签页')!).getByRole('option'));
  await user.type(box, '总结{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith({
      session_id: 'session',
      text: '总结',
      destination: { lane: 'next_turn' },
      attachments: [
        {
          kind: 'capture',
          name: 'Example.txt',
          text: 'Page "Example" (https://example.com/a; the user handed it to you as page `p1`: the browser tools act on it when given page "p1"; it stays the user\'s, who can take it back at any time, and browser.close does not close it) of the built-in browser as it was when this message was sent:\n\nHello from the page\n',
        },
      ],
    }),
  );
  // Handed over before the message goes, so the model's first step finds it.
  const handing = command.mock.calls.findIndex(([request]) => request.kind === 'hand-over');
  expect(command.mock.calls[handing]?.[0]).toEqual({
    kind: 'hand-over',
    sessionId: 'session',
    pageId: 'page-1',
  });
  expect(command.mock.invocationCallOrder[handing]).toBeLessThan(
    submit.mock.invocationCallOrder[0]!,
  );
  expect(command).toHaveBeenCalledWith({ kind: 'text', sessionId: 'session', pageId: 'page-1' });
  // A page closed meanwhile: the message waits, and says why.
  submit.mockClear();
  command.mockResolvedValue({ ok: false, message: '这个页面已关闭' });
  await user.type(box, '@');
  await waitFor(() => expect(group('浏览器标签页')).toBeTruthy());
  await user.click(within(group('浏览器标签页')!).getByRole('option'));
  await user.type(box, '再看{Enter}');
  await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('这个页面已关闭'));
  expect(screen.getByRole('alert').textContent).toContain(
    '「Example」没能交给模型：这个页面已关闭。消息没有发出，内容仍在输入框里。',
  );
  expect(submit).not.toHaveBeenCalled();
  expect(box.value).toBe('再看');
});

test("the model's own page is sent as its own, for the browser tools without a page id", async () => {
  browserTabs.add('session', 'model');
  browserStore.put({ ...browserStore.get('session', 'page-1')!, pageId: 'model', title: '' });
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '@');
  await waitFor(() => expect(group('浏览器标签页')).toBeTruthy());
  await user.click(within(group('浏览器标签页')!).getByRole('option', { name: /模型的页面/ }));
  await user.type(box, '看看{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith(
      expect.objectContaining({
        attachments: [
          {
            kind: 'capture',
            name: 'Example.txt',
            text: 'Page "Example" (https://example.com/a; your own page, which the browser tools act on without page) of the built-in browser as it was when this message was sent:\n\nHello from the page\n',
          },
        ],
      }),
    ),
  );
  expect(command).not.toHaveBeenCalledWith(expect.objectContaining({ kind: 'hand-over' }));
});

test('a terminal is read as its lines show, wrapped rows joined and the empty end left out', () => {
  const rows = [
    { text: 'PS C:\\w> echo a-long-', wrapped: false },
    { text: 'line', wrapped: true },
    { text: 'a-long-line', wrapped: false },
    { text: '', wrapped: false },
    { text: '', wrapped: false },
  ];
  const buffer = {
    length: rows.length,
    getLine: (y: number) => ({
      isWrapped: rows[y]!.wrapped,
      translateToString: () => rows[y]!.text,
    }),
  };
  expect(bufferText(buffer, 2000)).toBe('PS C:\\w> echo a-long-line\na-long-line');
  expect(bufferText(buffer, 1)).toBe('a-long-line');
});

test('"@" lists the MCP resources of the ready servers; one picked is read by Core when the message is sent', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '@');
  await waitFor(() => expect(group('MCP 资源')).toBeTruthy());
  expect(resources).toHaveBeenCalledWith({ session_id: 'session' });
  expect(options('MCP 资源')).toEqual(['Fixture notes笔记']);
  // A server that could not list its resources says so.
  expect(screen.getByText('文档：服务器没有启动')).toBeTruthy();
  await user.click(within(group('MCP 资源')!).getByRole('option'));
  expect(
    within(screen.getByRole('group', { name: '引用' })).getByText('Fixture notes'),
  ).toBeTruthy();
  await user.type(box, '按这份做{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith({
      session_id: 'session',
      text: '按这份做',
      destination: { lane: 'next_turn' },
      attachments: [
        {
          kind: 'mcp_resource',
          server_id: 'notes',
          uri: 'fixture://notes/readme',
          name: 'Fixture notes',
        },
      ],
    }),
  );
});
