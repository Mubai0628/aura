import { expect, test, type Page } from '@playwright/test';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, readdir, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { armReadOnlyWrite } from './s06/setup';
import { answer, finished, launch, pending, submit } from './s06/harness';
import { writeProvider, type ProviderRequest } from './s06-file-write-provider.mjs';
import { throughEntry } from './tool-entry';

const TOOL = 'user.org.aura.l04.fixture';

/** A real repo-built command package: one tool that prints a fixed line. */
async function commandPackage(root: string): Promise<string> {
  const repository = resolve(import.meta.dirname, '../../..');
  const manifest = JSON.parse(
    await readFile(
      join(repository, 'fixtures/tool-package/manifest-v1/valid/aura-stdio.json'),
      'utf8',
    ),
  );
  const executable = await readFile(join(repository, 'target/debug/aura-command-fixture.exe'));
  manifest.package_key = 'org.aura.l04';
  manifest.tools[0].runtime = {
    type: 'declarative_command',
    command: {
      type: 'argv',
      executable: 'bin/fixture.exe',
      arguments: [],
      environment: [],
      working_directory: { type: 'package_root' },
      credential_slot: null,
      success_exit_codes: [0],
    },
  };
  manifest.tools[0].model.result_contract = { type: 'utf8_text' };
  manifest.files = [
    {
      path: 'bin/fixture.exe',
      role: 'runtime_executable',
      byte_length: executable.length,
      sha256: { algorithm: 'sha256', hex: createHash('sha256').update(executable).digest('hex') },
    },
  ];
  const directory = join(root, 'package');
  await mkdir(join(directory, 'bin'), { recursive: true });
  await writeFile(join(directory, 'bin/fixture.exe'), executable);
  await writeFile(join(directory, 'aura-tool-package.json'), JSON.stringify(manifest));
  return directory;
}

/** The last durable tool result Core sent back to the model in this request. */
function lastTool(request: ProviderRequest): Record<string, unknown> | undefined {
  const message = request.messages
    .filter((value) => (value as { role?: string }).role === 'tool')
    .at(-1) as { content: string } | undefined;
  return message ? (JSON.parse(message.content) as Record<string, unknown>) : undefined;
}

/**
 * The model side of the scenario: find the package tool with `tools.search`, load it,
 * call it once. After the package is disabled it only searches and records the answer.
 */
async function packageProvider() {
  const phases = new Map<string, string>();
  const facts: { result?: Record<string, unknown> | undefined; foundAfterDisable?: boolean } = {};
  let disabled = false;
  const provider = await writeProvider({
    path: 'unused.txt',
    text: '',
    planAction(request) {
      const phase = phases.get(request.turn);
      const visible = request.tools.some((tool) =>
        (tool as { function: { name: string } }).function.name.startsWith(
          `${TOOL.replaceAll('.', '_')}__`,
        ),
      );
      if (disabled) {
        if (phase === undefined) {
          phases.set(request.turn, 'searched');
          return { tool: 'tools.search', arguments: { query: TOOL, limit: 50, cursor: null } };
        }
        if (phase === 'searched') {
          const page = (lastTool(request)?.outcome as { payload?: { value?: unknown } })?.payload
            ?.value as { results?: Array<{ canonical_name: string }> } | undefined;
          facts.foundAfterDisable =
            visible || (page?.results ?? []).some((hit) => hit.canonical_name === TOOL);
          phases.set(request.turn, 'done');
        }
        return { tool: null, arguments: {} };
      }
      if (phase === 'called') {
        facts.result = lastTool(request);
        return { tool: null, arguments: {} };
      }
      if (visible) {
        phases.set(request.turn, 'called');
        return { tool: TOOL, arguments: {} };
      }
      if (phase === 'searched') {
        const page = (lastTool(request)?.outcome as { payload?: { value?: unknown } })?.payload
          ?.value as { results?: Array<{ canonical_name: string; tool_id: string }> } | undefined;
        const hit = page?.results?.find((result) => result.canonical_name === TOOL);
        if (!hit) throw new Error('the installed package tool must be searchable');
        phases.set(request.turn, 'loaded');
        return { tool: 'tools.load', arguments: { tool_ids: [hit.tool_id], replace: [] } };
      }
      if (phase === 'loaded') {
        phases.set(request.turn, 'called');
        return throughEntry({ tool: TOOL, arguments: {} });
      }
      phases.set(request.turn, 'searched');
      return { tool: 'tools.search', arguments: { query: TOOL, limit: 50, cursor: null } };
    },
  });
  return { provider, facts, disable: () => (disabled = true) };
}

async function payloadCount(root: string) {
  const files = (await readdir(root, { recursive: true })).filter((file) =>
    file.endsWith('aura-v2.db'),
  );
  expect(files).toHaveLength(1);
  const db = new DatabaseSync(join(root, files[0]!), { readOnly: true });
  try {
    return Number(db.prepare('SELECT count(*) AS n FROM tool_package_payloads').get()!.n);
  } finally {
    db.close();
  }
}

async function packagesPanel(page: Page) {
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page
    .locator('.settings-navigation')
    .getByRole('button', { name: '工具', exact: true })
    .click();
  return page.getByRole('region', { name: '工具包', exact: true });
}

// eslint-disable-next-line no-empty-pattern -- actual Electron, Core and package files use isolated data.
test('L04 an imported package is verified, installed, called by the model, disabled and uninstalled', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l04-package-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const packageRoot = await commandPackage(root);
  const model = await packageProvider();
  const current = await launch(root);
  try {
    const { app, page } = current;
    await app.evaluate(({ dialog }, path) => {
      Reflect.set(dialog, 'showOpenDialog', async () => ({ canceled: false, filePaths: [path] }));
    }, packageRoot);
    const panel = await packagesPanel(page);
    await panel.getByRole('button', { name: '导入目录', exact: true }).click();
    const review = panel.getByRole('region', { name: '工具包版本' });
    await expect(review).toContainText('以你的身份运行');
    // The "runs as you" notice reads as text: one character per line made it hundreds of
    // pixels tall.
    const notice = review.getByRole('note');
    await expect(notice).toContainText('以你的 Windows');
    expect((await notice.boundingBox())?.height ?? Infinity).toBeLessThan(150);
    await review.getByRole('button', { name: '准备验证', exact: true }).click();
    await panel
      .getByRole('dialog', { name: '批准运行验证' })
      .getByRole('button', { name: '批准并运行验证', exact: true })
      .click();
    await expect(review).toContainText('验证通过', { timeout: 90_000 });
    await review.getByRole('button', { name: '安装：信任这个版本并启用', exact: true }).click();
    await expect(panel).toContainText('已安装 1.0.0', { timeout: 30_000 });

    // ReadOnly asks every time and offers no session answer for a package program.
    const setup = await armReadOnlyWrite(page, workspace, model.provider.url, {
      title: 'L04 package',
      text: '运行工具包里的工具',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const question = await pending(page, session);
    expect(question.operation.content.action_kind).toBe(TOOL);
    expect(question.backend.id).toBe('agent-native-package-v1');
    expect(question.approval.allowed_decisions).toEqual(['allow_once', 'deny']);
    await answer(page, session, 'allow_once');
    await finished(page, session);
    const outcome = model.facts.result?.outcome as {
      type: string;
      payload?: { type: string; value: unknown };
    };
    expect(outcome?.type, JSON.stringify(model.facts.result)).toBe('succeeded');
    expect(outcome.payload?.value).toBe('authorized command fixture\n');
    // The session's activity row states where the program ran.
    const rows = await page.evaluate(async (id) => {
      const activity = await window.aura.v1.readAgentToolActivity({
        session_id: id,
        before_call_id: null,
        limit: 20,
      });
      return activity.ok
        ? activity.value.calls.map((call) => [call.summary.display_name, call.display.summary])
        : 'failed';
    }, session);
    expect(rows).toContainEqual([
      TOOL,
      '执行成功；以当前 Windows 用户身份运行，Aura 不隔离它的文件写入',
    ]);

    // Disabling takes effect at once: the next turn neither sees nor finds the tool.
    const installed = panel.getByRole('region', { name: '已安装的工具包' });
    await panel.getByRole('button', { name: '管理', exact: true }).click();
    await installed.getByRole('button', { name: '停用', exact: true }).click();
    await installed
      .getByRole('dialog', { name: '确认工具包更改' })
      .getByRole('button', { name: '确认', exact: true })
      .click();
    await expect(installed).toContainText('已停用');
    model.disable();
    await submit(page, session, '再试一次');
    await finished(page, session);
    expect(model.facts.foundAfterDisable).toBe(false);

    // Uninstall removes it, and nothing references the content: it is collected.
    await installed.getByRole('button', { name: '卸载', exact: true }).click();
    await installed
      .getByRole('dialog', { name: '确认工具包更改' })
      .getByRole('button', { name: '确认', exact: true })
      .click();
    await expect
      .poll(async () =>
        page.evaluate(async () => {
          const list = await window.aura.v1.packages.invoke('packages.list', {});
          return list.ok ? list.value.packages.map((item) => item.installed ?? null) : 'failed';
        }),
      )
      .toEqual([null]);
    await expect.poll(async () => payloadCount(root), { timeout: 30_000 }).toBe(0);
    expect(model.provider.errors).toEqual([]);
  } finally {
    await current.app.close();
    await model.provider.close();
  }
});
