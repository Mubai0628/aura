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
  resources = vi.fn();
beforeEach(async () => {
  localStorage.clear();
  resetComposerDraftsForTests();
  resetTerminalStoreForTests();
  resetToolTabsForTests();
  browserTabs.resetForTests();
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
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
    request.kind === 'text' && request.pageId === 'page-1'
      ? {
          ok: true,
          text: {
            title: 'Example',
            url: 'https://example.com/a',
            text: 'Hello from the page',
            truncated: false,
          },
        }
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

test('a terminal picked from "@" is sent as what it shows, its last 2,000 lines', async () => {
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
          text: 'Terminal "终端 1" (PowerShell 7) as it was when this message was sent, up to its last 2,000 lines:\n\nPS C:\\w> cargo test\nerror[E0425]\n',
        },
      ],
    }),
  );
  await waitFor(() => expect(screen.queryByRole('group', { name: '引用' })).toBeNull());
});

test("a terminal the model opened is sent with its id, for the model's terminal tools", async () => {
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
});

test('a page picked from "@" is sent with its title, address and text; one that cannot be read is not sent', async () => {
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
          text: 'Page "Example" (https://example.com/a) of the built-in browser as it was when this message was sent:\n\nHello from the page\n',
        },
      ],
    }),
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
  expect(submit).not.toHaveBeenCalled();
  expect(box.value).toBe('再看');
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
