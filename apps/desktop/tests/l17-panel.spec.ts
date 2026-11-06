import { expect, test, type Page } from '@playwright/test';
import { mkdir, mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { finished, launch } from './s06/harness';
import { writeProvider, type ProviderRequest } from './s06-file-write-provider.mjs';
import { expectSecureSurface } from './electron-shell/renderer-api';
import { panelText, pluginRow, pluginSettings, scriptedDefaults, workspace } from './l17/setup';

// The plugin as the Skill's panel reference describes one: a manifest with `[panel]`,
// `session.read` and the window's notice and status entry, a page and its script in `panel/`, the
// session's numbers shown with textContent.
const MANIFEST = `schema_version = 2
name = "usage-panel"
version = "0.1.0"
description = "本会话用量"
capabilities = ["session.read", "notify", "status"]

[panel]
title = "本会话用量"
`;
const PAGE = `<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8" />
    <title>本会话用量</title>
  </head>
  <body>
    <p id="usage">读取中…</p>
    <script src="main.js"></script>
  </body>
</html>
`;
const SCRIPT = `const shown = document.getElementById('usage');
async function show() {
  const { usage } = await auraPanel.state();
  shown.textContent = \`请求 \${usage.requests} · 输入 \${usage.input_tokens} · 输出 \${usage.output_tokens}\`;
  await auraPanel.status(\`请求 \${usage.requests}\`);
}
auraPanel.notify('用量面板已打开');
show();
setInterval(show, 1000);
`;

type Step = { tool: string | null; arguments: Record<string, unknown> };

/** What the newest tool result in this request gave, as the model was sent it. */
function lastResult(request: ProviderRequest): Record<string, unknown> | undefined {
  const last = request.messages
    .filter((message) => (message as { role?: string }).role === 'tool')
    .at(-1) as { content?: string } | undefined;
  if (last === undefined) return undefined;
  const result = JSON.parse(String(last.content)) as {
    outcome?: { payload?: { value?: Record<string, unknown> } };
  };
  return result.outcome?.payload?.value;
}

/**
 * The model side of writing the plugin: it loads the Skill, writes the three files, checks the
 * folder and installs it in development, a step per request. A tool it does not see among its
 * tools is reached through the unified entry, as a model does.
 */
async function pluginWriter() {
  const write = (path: string, text: string): Step => ({
    tool: 'fs.write',
    arguments: { path, text, expected: { expected_kind: 'absent' } },
  });
  const folder = 'plugins/usage-panel';
  const steps: Step[] = [
    { tool: 'skills.load', arguments: { name: 'write-aura-plugin' } },
    write(`${folder}/aura-plugin.toml`, MANIFEST),
    write(`${folder}/panel/index.html`, PAGE),
    write(`${folder}/panel/main.js`, SCRIPT),
    { tool: 'plugins.validate', arguments: { path: folder } },
    { tool: 'plugins.dev_install', arguments: { path: folder } },
  ];
  const results: (Record<string, unknown> | undefined)[] = [];
  const nothing = { tool: null, arguments: {} };
  // The first turn writes the plugin; a later one only answers.
  let writing: string | undefined;
  const provider = await writeProvider({
    path: 'unused.txt',
    text: '',
    finalText: '面板插件已装成开发中，请在 设置 → 插件 里查看并信任。',
    planAction(request) {
      writing ??= request.turn;
      if (request.turn !== writing) return nothing;
      if (request.step > 0) results.push(lastResult(request));
      const step = steps[request.step];
      if (!step?.tool) return nothing;
      const prefix = `${step.tool.replaceAll('.', '_')}__`;
      const offered = (request.tools as { function: { name: string } }[]).some((tool) =>
        tool.function.name.startsWith(prefix),
      );
      return offered
        ? step
        : { tool: 'tools.call', arguments: { tool: step.tool, arguments: step.arguments } };
    },
  });
  return { provider, results };
}

/**
 * A conversation in the workspace in 工作区读写 on the scripted model, sent `text` as the user
 * sends it.
 */
async function conversation(page: Page, workspaceId: string, profile: string, text: string) {
  const id = await page.evaluate(
    async ({ workspaceId, profile, text }) => {
      const api = window.aura.v1;
      const session = await api.createSession({ workspace_id: workspaceId, title: '' });
      if (!session.ok) return JSON.stringify(session);
      const binding = await api.setSessionModelBinding({
        session_id: session.value.session_id,
        expected_revision: '0',
        model_profile_id: profile,
      });
      if (!binding.ok) return JSON.stringify(binding);
      const sent = await api.submitAgentMessage({
        session_id: session.value.session_id,
        destination: { lane: 'next_turn' },
        text,
      });
      return sent.ok ? session.value.session_id : JSON.stringify(sent);
    },
    { workspaceId, profile, text },
  );
  expect(id).toMatch(/^ses_/u);
  return id;
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core and the plugin page, a TEMP workspace and isolated settings; scripted loopback model only.
test('L17 a panel plugin written by the model with the Skill is installed in development, trusted by the user and shows the usage', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l17-panel-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const work = join(root, 'work');
  await mkdir(work);
  const model = await pluginWriter();
  const { app, page } = await launch(root);
  try {
    await expectSecureSurface(page);
    const profile = await scriptedDefaults(page, model.provider.url, 'workspace_read_write');
    const space = await workspace(page, work, 'Plugins');
    const session = await conversation(
      page,
      space,
      profile,
      '照写插件的 Skill 写一个“本会话用量”面板并装上',
    );
    await finished(page, session);

    // The Skill came with Aura; the folder checked out; it was installed in development and
    // nothing was trusted by the model.
    const [loaded, , , , checked, installed] = model.results;
    expect(JSON.stringify(loaded)).toContain('reference/panel.md');
    expect(checked, JSON.stringify(model.results)).toMatchObject({
      name: 'usage-panel',
      installable: true,
      brings: { panels: 1 },
      problems: [],
    });
    expect(installed).toMatchObject({ installed: true, state: 'not_trusted' });
    expect(await readFile(join(work, 'plugins/usage-panel/panel/main.js'), 'utf8')).toBe(SCRIPT);
    const plugins = await pluginSettings(page);
    const row = pluginRow(plugins, 'usage-panel');
    await expect(row).toContainText('开发中');
    await expect(row).toContainText('未信任，内容都不提供');

    // The user reviews and trusts it; its panel is then offered in the conversation's sidebar.
    await row.getByRole('button', { name: '查看并信任' }).click();
    const dialog = page.getByRole('dialog', { name: 'usage-panel' });
    await dialog
      .getByRole('region', { name: 'usage-panel 的内容' })
      .getByRole('button', { name: '信任并启用' })
      .click();
    await expect(row).toContainText('可用');

    // Exported as a ZIP file where the user chooses, offered under the plugin's name and version.
    const zip = join(root, 'exported.zip');
    await app.evaluate(({ dialog }, path) => {
      const asked: unknown[] = [];
      Reflect.set(globalThis, 'auraExportAsked', asked);
      Reflect.set(dialog, 'showSaveDialog', async (_owner: unknown, options: unknown) => {
        asked.push(options);
        return { canceled: false, filePath: path };
      });
    }, zip);
    await dialog.getByRole('button', { name: '导出…' }).click();
    await expect(dialog.getByRole('status').filter({ hasText: '已导出' })).toBeVisible();
    const asked = await app.evaluate(() => Reflect.get(globalThis, 'auraExportAsked') as unknown);
    expect(asked).toMatchObject([{ defaultPath: 'usage-panel-0.1.0.zip' }]);
    expect((await readFile(zip)).subarray(0, 2).toString('latin1')).toBe('PK');
    await dialog.getByRole('button', { name: '关闭' }).click();

    await page.getByRole('button', { name: '返回会话' }).click();
    await page.locator('.session-row').first().click();
    await page.getByRole('button', { name: '打开工具' }).click();
    await page
      .getByRole('menu', { name: '选择工具' })
      .getByRole('menuitem', { name: /本会话用量/u })
      .click();
    // The notice its page shows on loading is in Aura's notices, under the plugin's name, for the
    // few seconds an information notice stays.
    const notice = page
      .getByLabel('系统消息')
      .getByRole('status')
      .filter({ hasText: '用量面板已打开' });
    await expect(notice).toContainText('插件 usage-panel', { timeout: 30_000 });
    const requests = model.provider.requests.length;
    await expect
      .poll(() => panelText(app, 'usage-panel'), { timeout: 30_000 })
      .toBe(`请求 ${requests} · 输入 ${requests * 1000} · 输出 ${requests * 100}`);
    // Its entry, in its row on Settings → 插件, while its panel stays open in the conversation.
    const entry = () =>
      page.evaluate(
        async () =>
          (await window.aura.v1.pluginUi?.statuses())?.find((each) => each.plugin === 'usage-panel')
            ?.text,
      );
    const line = pluginRow(await pluginSettings(page), 'usage-panel').locator(
      '.plugin-status-line',
    );
    await expect(line).toContainText(`请求 ${requests}`);
    await page.getByRole('button', { name: '返回会话' }).click();

    // The next turn's request shows there while the panel stays open.
    await page.evaluate(
      (session_id) =>
        window.aura.v1.submitAgentMessage({
          session_id,
          destination: { lane: 'next_turn' },
          text: '好的',
        }),
      session,
    );
    await expect.poll(() => model.provider.requests.length).toBeGreaterThan(requests);
    await finished(page, session);
    const more = model.provider.requests.length;
    await expect
      .poll(() => panelText(app, 'usage-panel'), { timeout: 30_000 })
      .toBe(`请求 ${more} · 输入 ${more * 1000} · 输出 ${more * 100}`);
    await expect.poll(entry).toContain(`请求 ${more}`);

    // Closing its tab closes the panel, and its status entry goes with it.
    await page.getByRole('button', { name: '关闭本会话用量' }).click();
    await expect.poll(entry).toBeUndefined();
    await expect(
      pluginRow(await pluginSettings(page), 'usage-panel').locator('.plugin-status-line'),
    ).toHaveCount(0);
    await expect.poll(() => panelText(app, 'usage-panel')).toBe('');
    expect(model.provider.errors).toEqual([]);
  } finally {
    await app.close();
    await model.provider.close();
  }
});
