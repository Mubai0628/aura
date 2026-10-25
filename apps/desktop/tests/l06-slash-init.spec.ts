import { expect, test, type Page } from '@playwright/test';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { absent, answer, finished, launch, pending, submit } from './s06/harness';
import {
  writeProvider,
  type ProviderAction,
  type ProviderRequest,
} from './s06-file-write-provider.mjs';
import { throughEntry } from './tool-entry';
import { expectSecureSurface } from './electron-shell/renderer-api';

function lastTool(request: ProviderRequest): Record<string, unknown> | undefined {
  const last = request.messages
    .filter((value) => (value as { role?: string }).role === 'tool')
    .at(-1) as { content: string } | undefined;
  return last ? (JSON.parse(last.content) as Record<string, unknown>) : undefined;
}

const exposed = (request: ProviderRequest, tool: string) =>
  request.tools.some((value) =>
    (value as { function: { name: string } }).function.name.startsWith(
      `${tool.replaceAll('.', '_')}__`,
    ),
  );

/** Calls the current action's tool once per turn, finding and loading it first if needed. */
function scripted(current: () => ProviderAction) {
  const phases = new Map<string, string>();
  return (request: ProviderRequest): ProviderAction => {
    const action = current();
    const phase = phases.get(request.turn);
    if (!action.tool || phase === 'called') return { tool: null, arguments: {} };
    if (exposed(request, action.tool)) {
      phases.set(request.turn, 'called');
      return action;
    }
    if (phase === 'searched') {
      const hits = (
        lastTool(request)?.outcome as {
          payload?: { value: { results: Array<{ canonical_name: string; tool_id: string }> } };
        }
      )?.payload?.value.results;
      const hit = hits?.find((value) => value.canonical_name === action.tool);
      if (!hit) throw new Error(`${action.tool} must be discoverable`);
      phases.set(request.turn, 'loaded');
      return { tool: 'tools.load', arguments: { tool_ids: [hit.tool_id], replace: [] } };
    }
    if (phase === 'loaded') {
      phases.set(request.turn, 'called');
      return throughEntry(action);
    }
    phases.set(request.turn, 'searched');
    return { tool: 'tools.search', arguments: { query: action.tool, limit: 50, cursor: null } };
  };
}

async function composer(page: Page) {
  await page.locator('.session-row').first().click();
  const box = page.getByRole('textbox', { name: '输入消息' });
  await expect(box).toBeVisible();
  await box.click();
  return box;
}

const init = {
  tool: 'workspace_init.complete',
  arguments: {
    language: 'english',
    expected: { expected_kind: 'absent' },
    project_structure: 'The demo has one crate.',
    commands: {
      entries: [
        {
          command: 'cargo test --workspace',
          purpose: 'Run every Rust test',
          shell: 'powershell',
          evidence: { kind: 'repository_defined', path: 'README.md' },
        },
      ],
    },
    coding_style: 'Format Rust with rustfmt.',
    testing: 'Write a failing test before the fix.',
    commits_and_pull_requests: 'One complete change per commit.',
  },
};

const review = {
  tool: 'commands.save',
  arguments: {
    name: 'review',
    title: '审查本次改动',
    description: '列出本次改动的风险',
    argument_hint: '范围（可选）',
    template: '请审查 $ARGUMENTS 的改动，列出风险。',
    expected: { kind: 'absent' },
  },
};

// eslint-disable-next-line no-empty-pattern -- real Electron/Core and TEMP files; scripted loopback model only.
test('L06 typed slash commands, /init with one ReadOnly answer, and a workspace command made by the model', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l06-init-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  await writeFile(
    join(workspace, 'README.md'),
    '# Demo\n\nRun `cargo test --workspace` before a commit.\n',
  );
  let action: ProviderAction = { tool: null, arguments: {} };
  const model = await writeProvider({
    path: 'unused',
    text: '',
    planAction: scripted(() => action),
  });
  const { app, page } = await launch(root);
  try {
    await expectSecureSurface(page);
    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L06 Init',
      text: '你好',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await finished(page, session);
    const box = await composer(page);

    // Only a typed slash opens the list; picking an immediate command runs it.
    await page.keyboard.type('/');
    const menu = page.getByRole('listbox', { name: '命令' });
    await expect(menu).toBeVisible();
    await page.keyboard.type('term');
    await page.keyboard.press('Enter');
    await expect(menu).toHaveCount(0);
    // /terminal shows the conversation's terminal tab, which opens the default terminal.
    await expect(page.getByRole('tab', { name: '终端 1', exact: true })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    await expect(box).toHaveValue('');

    // /init becomes a block, starts the init task, and asks once with allow once or deny.
    action = init;
    await box.click();
    await page.keyboard.type('/ini');
    // This window learns that the first turn finished a moment after Core does; until then
    // /init is greyed ("当前任务结束…") and Enter on it does nothing.
    await expect(page.getByRole('option', { name: /^\/init/u })).toHaveAttribute(
      'aria-disabled',
      'false',
    );
    await page.keyboard.press('Enter');
    await expect(page.getByRole('group', { name: '命令块' })).toContainText('/init');
    await page.keyboard.type('重点写测试命令');
    await page.keyboard.press('Enter');
    const question = await pending(page, session);
    expect(question.operation.content.action_kind).toBe('workspace_init.complete');
    expect(question.approval.allowed_decisions).toEqual(['allow_once', 'deny']);
    // The card shows the model's explanation; what /init writes is under 查看详情.
    await page.locator('section[aria-label="当前任务审批"]').getByText('查看详情').click();
    await expect(page.getByLabel('初始化写入内容')).toBeVisible();
    expect(await absent(join(workspace, 'AGENTS.md'))).toBe(true);
    await answer(page, session, 'allow_once');
    await finished(page, session);
    const written = await readFile(join(workspace, 'AGENTS.md'), 'utf8');
    expect(written).toContain('## Build, Test, and Development Commands');
    expect(written).toContain('Source: defined in `README.md`');
    expect(JSON.stringify(model.requests)).toContain('This is a /init task');

    // A command the model drafts asks once, then appears in the menu as a workspace command.
    action = review;
    await submit(page, session, '为这个项目保存一个审查改动的命令');
    const save = await pending(page, session);
    expect(save.operation.content.action_kind).toBe('commands.save');
    expect(save.approval.allowed_decisions).toEqual(['allow_once', 'deny']);
    await page.locator('section[aria-label="当前任务审批"]').getByText('查看详情').click();
    await expect(page.getByLabel('命令写入内容')).toBeVisible();
    await answer(page, session, 'allow_once');
    await finished(page, session);
    expect(await readFile(join(workspace, '.aura/commands/review/template.md'), 'utf8')).toBe(
      '请审查 $ARGUMENTS 的改动，列出风险。',
    );
    action = { tool: null, arguments: {} };
    await box.click();
    await page.keyboard.type('/rev');
    const option = page.getByRole('option', { name: /审查本次改动/u });
    await expect(option).toContainText('工作区命令');
    await page.keyboard.press('Enter');
    await expect(box).toHaveValue('请审查  的改动，列出风险。');
  } finally {
    await app.close();
    await model.close();
  }
});

// eslint-disable-next-line no-empty-pattern -- real Electron/Core, TEMP files and the real clipboard (restored).
test('L06 pasted slash text never opens the menu and reaches the model exactly as written', async ({}, info) => {
  // The clipboard belongs to whoever is at the desk: like the terminal clipboard test,
  // this runs only with AURA_E2E_SYSTEM_CLIPBOARD=1 and puts back whatever the clipboard held.
  test.skip(
    process.env.AURA_E2E_SYSTEM_CLIPBOARD !== '1',
    'uses the real Windows clipboard; set AURA_E2E_SYSTEM_CLIPBOARD=1 to run it',
  );
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l06-paste-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const model = await writeProvider({
    path: 'unused',
    text: '',
    planAction: () => ({ tool: null, arguments: {} }),
  });
  const { app, page } = await launch(root);
  // Electron writes only newly constructed ClipboardItems, so keep each item's data in Main.
  await app.evaluate(async ({ clipboard }) => {
    const items = await clipboard.read();
    const saved = await Promise.all(
      items.map(async (item) =>
        Object.fromEntries(
          await Promise.all(item.types.map(async (type) => [type, await item.getType(type)])),
        ),
      ),
    );
    Object.assign(globalThis, { auraTestClipboard: saved });
  });
  try {
    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L06 Paste',
      text: '你好',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await finished(page, session);
    const box = await composer(page);
    await app.evaluate(({ clipboard }) => clipboard.writeText('/plan 先做计划'));
    await page.keyboard.press('Control+V');
    await expect(box).toHaveValue('/plan 先做计划');
    await expect(page.getByRole('listbox', { name: '命令' })).toHaveCount(0);
    await expect(page.getByRole('group', { name: '命令块' })).toHaveCount(0);
    const before = model.requests.length;
    await page.keyboard.press('Enter');
    await expect.poll(() => model.requests.length).toBeGreaterThan(before);
    await finished(page, session);
    const user = (model.requests.at(-1)!.messages as Array<{ role: string; content: unknown }>)
      .filter((message) => message.role === 'user')
      .at(-1);
    expect(JSON.stringify(user?.content)).toContain('/plan 先做计划');
    await expect(box).toHaveValue('');
  } finally {
    await app
      .evaluate(async ({ clipboard, ClipboardItem }) => {
        const { auraTestClipboard: saved } = globalThis as {
          auraTestClipboard?: ConstructorParameters<typeof ClipboardItem>[0][];
        };
        if (!saved) return;
        await (saved.length > 0
          ? clipboard.write(saved.map((entry) => new ClipboardItem(entry)))
          : clipboard.clear());
      })
      .catch((error: unknown) => console.error('restoring the clipboard failed', error));
    await app.close();
    await model.close();
  }
});

// eslint-disable-next-line no-empty-pattern -- real Electron/Core and TEMP files; scripted loopback model only.
test('L06 a ReadOnly memory answer is listed in session grants and revoking it makes the next write ask', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l06-memory-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const memory = (path: string): ProviderAction => ({
    tool: 'memory.create',
    arguments: {
      scope: 'project',
      items: [
        {
          path,
          category: 'rules',
          body: '# 临时测试记忆\n只在本机夹具里。',
          tags: [],
          confidence: 1,
          importance: 3,
          recall_weight: 1,
          related_ids: [],
          supersedes_ids: [],
          conflicts_with_ids: [],
        },
      ],
    },
  });
  let action = memory('rules/first.md');
  const model = await writeProvider({
    path: 'unused',
    text: '',
    planAction: scripted(() => action),
  });
  const { app, page } = await launch(root);
  try {
    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L06 Memory',
      text: '记下第一条约定',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    expect((await pending(page, session)).operation.content.action_kind).toBe('memory.create');
    await answer(page, session, 'allow_session');
    await finished(page, session);
    await composer(page);
    await page.keyboard.type('/perm');
    await page.keyboard.press('Enter');
    await page.getByRole('menuitem', { name: /会话授权管理/u }).click();
    const grants = page.getByRole('region', { name: '会话记忆批准' });
    await expect(grants).toContainText('本项目记忆库 · 写入记忆 · 生效中');
    await grants.getByRole('button', { name: '撤销记忆批准' }).click();
    await expect(grants).toContainText('本项目记忆库 · 写入记忆 · 已撤销');
    await page.getByRole('button', { name: '关闭授权管理' }).click();

    action = memory('rules/second.md');
    await submit(page, session, '再记一条约定');
    const again = await pending(page, session);
    expect(again.operation.content.action_kind).toBe('memory.create');
    expect(await absent(join(workspace, '.memories/rules/second.md'))).toBe(true);
    await answer(page, session, 'deny');
  } finally {
    await app.close();
    await model.close();
  }
});
