import { expect, test, type Page } from '@playwright/test';
import { copyFile, mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { finished, launch, submit } from './s06/harness';
import {
  writeProvider,
  type ProviderAction,
  type ProviderRequest,
} from './s06-file-write-provider.mjs';
import { expectSecureSurface } from './electron-shell/renderer-api';

const reason = 'E2E 守卫：不许读 notes.txt';

/** A Hook package in TEMP: the repository's Hook test program, refusing `fs.read` with a reason. */
async function guardHook(root: string): Promise<string> {
  const repository = resolve(import.meta.dirname, '../../..');
  const directory = join(root, 'hooks', 'guard');
  await mkdir(join(directory, 'bin'), { recursive: true });
  await copyFile(
    join(repository, 'target/debug/aura-hook-fixture.exe'),
    join(directory, 'bin/hook.exe'),
  );
  await writeFile(
    join(directory, 'hook.toml'),
    [
      'schema_version = 1',
      'name = "guard"',
      'version = "1.0.0"',
      'description = "不让模型读 notes.txt"',
      'events = ["tool.before_call"]',
      '[command]',
      'executable = "bin/hook.exe"',
      `arguments = ['answer', '{"decision":"block","reason":"${reason}"}']`,
      '[match]',
      'tools = ["fs.read"]',
      '',
    ].join('\n'),
  );
  return directory;
}

const read: ProviderAction = {
  tool: 'fs.read',
  arguments: { path: 'notes.txt', range: { range_kind: 'full' }, explicit_encoding: null },
};

/** The text of the last tool result Core sent to the model in this request. */
function lastTool(request: ProviderRequest): string {
  const last = request.messages
    .filter((value) => (value as { role?: string }).role === 'tool')
    .at(-1) as { content: string } | undefined;
  return last?.content ?? '';
}

async function hookSettings(page: Page) {
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page
    .locator('.settings-navigation')
    .getByRole('button', { name: 'Hook', exact: true })
    .click();
  return page.getByRole('region', { name: 'Hook 设置' });
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core, TEMP Hook files and settings; scripted loopback model only.
test('L07 a trusted Hook blocks a tool call with its reason and stops running once its files change', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l07-hooks-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  await writeFile(join(workspace, 'notes.txt'), 'hook-free notes');
  const hook = await guardHook(root);
  // Every turn reads notes.txt once and then answers.
  const model = await writeProvider({
    path: 'unused',
    text: '',
    planAction: (request) => (request.step === 0 ? read : { tool: null, arguments: {} }),
  });
  const { app, page } = await launch(root);
  try {
    await expectSecureSurface(page);
    // Main's folder dialog answers with the TEMP package; nothing else is stubbed.
    await app.evaluate(({ dialog }, path) => {
      Reflect.set(dialog, 'showOpenDialog', async () => ({ canceled: false, filePaths: [path] }));
    }, hook);

    // Settings → Hook: add, preview what runs, then trust exactly that content.
    const settings = await hookSettings(page);
    await settings.getByRole('button', { name: '添加 Hook…' }).click();
    const preview = settings.getByRole('region', { name: '预览 Hook guard' });
    // The one sentence about running as you is where you trust a Hook.
    await expect(preview.getByRole('note')).toContainText('以你的身份');
    await expect(preview).toContainText('工具调用前');
    await expect(preview).toContainText('只对这些工具：fs.read');
    await expect(preview).toContainText('运行包内程序 bin/hook.exe');
    await preview.getByRole('button', { name: '信任并启用' }).click();
    const order = settings.getByRole('list', { name: 'Hook 执行顺序' });
    await expect(order.getByRole('listitem')).toHaveCount(1);
    await expect(order).toContainText('已启用');
    await page.getByRole('button', { name: '返回会话' }).click();

    // The model reads notes.txt; the Hook stops the call and the model is told which and why.
    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L07 Hooks',
      text: '读一下 notes.txt',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await finished(page, session);
    const told = lastTool(model.requests[1]!);
    expect(told).toContain(`被 Hook guard 阻止：${reason}`);
    expect(told).not.toContain('hook-free notes');
    const activity = await page.evaluate(async (session_id) => {
      const result = await window.aura.v1.readAgentToolActivity({
        session_id,
        before_call_id: null,
        limit: 10,
      });
      if (!result.ok) throw new Error(JSON.stringify(result.error));
      return result.value.calls.map((call) => call.display);
    }, session);
    expect(activity[0]?.summary).toBe('未执行：被 Hook 阻止');
    expect(activity[0]?.details.join('\n')).toContain(`被 Hook guard 阻止：${reason}`);
    await page.locator('.session-row').first().click();
    await expect(page.getByText(`被 Hook guard 阻止（工具调用前）：${reason}`)).toBeVisible();
    await expect(page.getByText('Hook：调用 1 次，成功 0，失败 0，阻止 1')).toBeVisible();

    // A changed file withdraws the trust: the settings page says so and the Hook stops running.
    const manifest = join(hook, 'hook.toml');
    await writeFile(
      manifest,
      (await readFile(manifest, 'utf8')).replace('不让模型读 notes.txt', '改过的说明'),
    );
    const again = await hookSettings(page);
    await expect(again.getByRole('list', { name: 'Hook 执行顺序' })).toContainText(
      '内容已变化，需要重新信任',
    );
    await expect(again.getByRole('button', { name: '查看变化并重新信任' })).toBeVisible();
    await expect(again.getByRole('button', { name: '停用' })).toHaveCount(0);
    await page.getByRole('button', { name: '返回会话' }).click();
    await submit(page, session, '再读一次 notes.txt');
    await finished(page, session);
    expect(lastTool(model.requests.at(-1)!)).toContain('hook-free notes');
    // The changed package was not started: its record says so, and a user Hook fails open.
    const calls = await page.evaluate(async () => {
      const result = await window.aura.v1.listHookInvocations({
        path: null,
        turn_task_id: null,
        limit: 50,
      });
      if (!result.ok) throw new Error(JSON.stringify(result.error));
      return result.value.invocations.map((call) => [call.outcome, call.failure ?? null]);
    });
    expect(calls).toEqual([
      ['failed', 'content_changed'],
      ['blocked', null],
    ]);
    await page.locator('.session-row').first().click();
    await expect(page.getByText('Hook：调用 1 次，成功 0，失败 1，阻止 0')).toBeVisible();
  } finally {
    await app.close();
    await model.close();
  }
});
