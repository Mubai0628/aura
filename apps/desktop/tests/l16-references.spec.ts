import { expect, test, type Page } from '@playwright/test';
import { copyFile, mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { finished, launch } from './s06/harness';
import { writeProvider, type ProviderRequest } from './s06-file-write-provider.mjs';
import { expectSecureSurface } from './electron-shell/renderer-api';
import { openTerminalTab } from './l10-8/terminal-tabs';

const README = '# L16\n\nE2E-REF-README: the release code is RF-21.\n';
type Mode = 'setup' | 'path' | 'terminal' | 'resource';

/** The last durable tool result Core sent back to the model in this request. */
function lastTool(request: ProviderRequest): Record<string, unknown> | undefined {
  const message = request.messages
    .filter((value) => (value as { role?: string }).role === 'tool')
    .at(-1) as { content: string } | undefined;
  return message ? (JSON.parse(message.content) as Record<string, unknown>) : undefined;
}

type Payload = { payload?: { value?: { text?: string } } };
const readText = (request: ProviderRequest) =>
  (lastTool(request)?.outcome as Payload | undefined)?.payload?.value?.text;

/** The newest user message as the model reads it. */
function userText(request: ProviderRequest): string {
  const user = request.messages
    .filter((value) => (value as { role?: string }).role === 'user')
    .at(-1) as { content?: unknown } | undefined;
  const content = user?.content;
  if (typeof content === 'string') return content;
  return Array.isArray(content)
    ? content.map((part) => String((part as { text?: string }).text ?? '')).join('')
    : '';
}

/** The path of the external file the message attaches, as the model is told it. */
function attachedPath(text: string): string | undefined {
  const lead = 'external file `';
  const start = text.indexOf(lead);
  if (start < 0) return undefined;
  const from = start + lead.length;
  return text.slice(from, text.indexOf('`', from));
}

/**
 * The model side: the first step of each turn records the message it was sent and reads what
 * it refers to (the workspace file, or the kept file it attaches) with `fs.read`; the next step
 * records what the read gave and answers.
 */
async function referenceProvider() {
  const phases = new Map<string, string>();
  const facts: { sent: Partial<Record<Mode, string>>; read: Partial<Record<Mode, string>> } = {
    sent: {},
    read: {},
  };
  let mode: Mode = 'setup';
  const done = { tool: null, arguments: {} };
  const provider = await writeProvider({
    path: 'unused.txt',
    text: '',
    planAction(request) {
      const phase = phases.get(request.turn);
      phases.set(request.turn, 'done');
      const read = phase === 'read' ? readText(request) : undefined;
      if (read !== undefined) facts.read[mode] = read;
      if (phase !== undefined) return done;
      const sent = userText(request);
      facts.sent[mode] = sent;
      const path = mode === 'path' ? 'README.md' : attachedPath(sent);
      if (mode === 'setup' || !path) return done;
      phases.set(request.turn, 'read');
      return {
        tool: 'fs.read',
        arguments: { path, range: { range_kind: 'full' }, explicit_encoding: null },
      };
    },
  });
  return { provider, facts, next: (next: Mode) => (mode = next) };
}

/** The repository's test MCP server offering two resources, saved and trusted for this test. */
async function resourceServer(page: Page, root: string) {
  const repository = resolve(import.meta.dirname, '../../..');
  const exe = join(root, 'mcp', 'fixture-server.exe');
  await mkdir(join(root, 'mcp'), { recursive: true });
  await copyFile(join(repository, 'target/debug/aura-mcp-fixture.exe'), exe);
  const saved = await page.evaluate(async (command) => {
    const api = window.aura.v1;
    const listed = await api.listMcpServers({});
    if (!listed.ok) return listed;
    const server = {
      id: 'notes',
      display_name: '笔记',
      transport: { type: 'stdio', command, arguments: ['--resources'] },
      startup_timeout_ms: 10_000,
      call_timeout_ms: 10_000,
    };
    const saved = await api.saveMcpServer({
      server,
      replaces: null,
      expected_revision: listed.value.revision,
    } as Parameters<typeof api.saveMcpServer>[0]);
    if (!saved.ok) return saved;
    const now = await api.listMcpServers({});
    if (!now.ok) return now;
    return api.trustMcpServer({
      id: 'notes',
      expected_fingerprint: now.value.servers[0]!.fingerprint!,
      expected_revision: now.value.revision,
    });
  }, exe);
  expect(saved.ok, JSON.stringify(saved)).toBe(true);
  await expect
    .poll(
      async () =>
        page.evaluate(async () => {
          const listed = await window.aura.v1.listMcpServers({});
          return listed.ok ? listed.value.servers[0]?.state : 'failed';
        }),
      { timeout: 60_000 },
    )
    .toBe('ready');
}

/** Types into the composer and sends what is there. */
async function send(page: Page, text: string) {
  await page.getByRole('textbox', { name: '输入消息' }).click();
  await page.keyboard.type(text);
  await page.keyboard.press('Enter');
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core, a TEMP workspace, the repo's MCP server in TEMP and a TEMP profile; scripted loopback model only.
test('L16 "@" refers to a workspace file, a terminal and an MCP resource; the model reads each in ReadOnly without a question', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l16-references-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  await writeFile(join(workspace, 'README.md'), README);
  const model = await referenceProvider();
  const { app, page } = await launch(root);
  try {
    await expectSecureSurface(page);
    const setup = await armReadOnlyWrite(page, workspace, model.provider.url, {
      title: 'L16 引用',
      text: '你好',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await finished(page, session);
    await page.locator('.session-row').first().click();
    const box = page.getByRole('textbox', { name: '输入消息' });

    // A workspace file picked from the list is written as "@path"; Core sends it as a reference
    // the model reads from the workspace.
    model.next('path');
    await box.click();
    await page.keyboard.type('@READ');
    const files = page.getByRole('listbox', { name: '文件和文件夹' });
    await expect(files.getByRole('option', { name: 'README.md' })).toBeVisible({ timeout: 30_000 });
    await page.keyboard.press('Enter');
    await expect(box).toHaveValue('@README.md ');
    await page.keyboard.type('里写的代号是什么');
    await page.keyboard.press('Enter');
    await expect.poll(() => model.facts.read.path ?? '', { timeout: 90_000 }).toContain('RF-21');
    await finished(page, session);
    expect(model.facts.sent.path).toContain('workspace file `README.md`, not included');
    await expect(page.getByText('引用：README.md')).toBeVisible();

    // A terminal of this conversation, as it shows when the message is sent: a file of the
    // conversation the model reads by its path without a question.
    await openTerminalTab(page);
    await expect(page.locator('.xterm')).toHaveCount(1, { timeout: 30_000 });
    await page.locator('.terminal-host').click();
    await page.keyboard.type("Write-Output ('E2E-' + 'TERM-77')");
    await page.keyboard.press('Enter');
    await expect(page.locator('.xterm-accessibility-tree')).toContainText('E2E-TERM-77', {
      timeout: 15_000,
    });
    model.next('terminal');
    await box.click();
    await page.keyboard.type('@');
    const terminals = page.getByRole('listbox', { name: '终端' });
    await terminals.getByRole('option', { name: /终端 1/u }).click();
    const chips = page.getByRole('group', { name: '引用' });
    await expect(chips).toContainText('终端 1');
    await send(page, '终端里打印了什么');
    await expect
      .poll(() => model.facts.read.terminal ?? '', { timeout: 90_000 })
      .toContain('E2E-TERM-77');
    await finished(page, session);
    expect(model.facts.sent.terminal).toContain('v2-message-captures');
    expect(model.facts.read.terminal).toContain('Terminal "终端 1" (PowerShell 7)');
    await expect(chips).toHaveCount(0);
    await expect(page.getByRole('list', { name: '发送的文件' }).last()).toContainText('终端 1.txt');

    // An MCP resource of a ready server, read by Core when the message is sent and kept the same
    // way; the server is started for this conversation only when the list is opened.
    await resourceServer(page, root);
    model.next('resource');
    await box.click();
    await page.keyboard.type('@');
    // Both of the server's resources, by title or name, with the server's name beside them.
    const resources = page.getByRole('listbox', { name: 'MCP 资源' });
    await expect(resources.getByRole('option')).toHaveText(['Fixture notes笔记', 'logo笔记'], {
      timeout: 30_000,
    });
    await resources.getByRole('option', { name: /Fixture notes/u }).click();
    await expect(chips).toContainText('Fixture notes');
    await send(page, '按这份笔记做');
    await expect
      .poll(() => model.facts.read.resource ?? '', { timeout: 90_000 })
      .toContain('The first line.');
    await finished(page, session);
    expect(model.facts.read.resource).toContain(
      'MCP resource "Fixture notes" (fixture://notes/readme) of server "笔记"',
    );
    await expect(page.getByRole('list', { name: '发送的文件' }).last()).toContainText(
      'Fixture notes.txt',
    );
    expect(model.provider.errors).toEqual([]);
  } finally {
    await app.close();
    await model.provider.close();
  }
});
