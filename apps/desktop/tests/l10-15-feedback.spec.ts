import { expect, test, type ElectronApplication, type Page } from '@playwright/test';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { launch } from './s06/harness';
import { writeProvider, type ProviderAction } from './s06-file-write-provider.mjs';
import { answered } from './l10-14/conversation';

// In the real window with the real Core; only the model is a local script.
// A file the model names with a Markdown link to its path is the same file link as one it names
// in inline code, not "name [blocked]". The thinking level sits beside the model and goes with the
// model's next request. With many models the menu lists the providers and opens one's models
// beside it, both lists inside the window.

const CODEX_MODELS = 14;

/** Each turn reads the file first; the provider sends its final words after a tool result. */
const read: ProviderAction = {
  tool: 'fs.read',
  arguments: { path: 'subway-runner.html', range: { range_kind: 'full' }, explicit_encoding: null },
};

async function width(app: ElectronApplication, pixels: number, height: number) {
  await app.evaluate(
    ({ BrowserWindow }, [pixels, height]) => {
      BrowserWindow.getAllWindows()[0]!.setSize(pixels!, height!);
    },
    [pixels, height],
  );
}

/** A second provider with many models, as gateway providers often have. */
async function manyModels(page: Page, url: string) {
  const made = await page.evaluate(
    async ({ url, count }) => {
      const api = window.aura.v1;
      const settings = await api.getModelSettings();
      if (!settings.ok) return 'settings';
      const provider = await api.createModelProvider({
        expected_revision: settings.value.models_revision,
        input: { display_name: 'Codex', source: { type: 'custom', catalog_provider_id: null } },
      });
      if (!provider.ok) return `provider ${JSON.stringify(provider.error)}`;
      const endpoint = await api.createModelEndpoint({
        expected_revision: provider.value.revision,
        provider_id: provider.value.value.id,
        display_name: '1',
        api_format: 'open_ai_chat_completions',
        base_url: url,
        discovery_mode: { type: 'disabled' },
        chat_output_token_field: 'max_tokens',
        chat_instruction_role: 'system',
        auth_scheme: 'none',
        openai_organization_id: null,
        openai_project_id: null,
        anthropic_workspace_id: null,
        timeouts: {
          connect_ms: 5000,
          first_event_ms: 10_000,
          stream_idle_ms: 10_000,
          total_ms: 60_000,
        },
        max_parallel_model_requests: 2,
      });
      if (!endpoint.ok) return `endpoint ${JSON.stringify(endpoint.error)}`;
      let revision = endpoint.value.revision;
      for (let n = 0; n < count; n += 1) {
        const profile = await api.createModelProfile({
          expected_revision: revision,
          input: {
            endpoint_id: endpoint.value.value.id,
            provider_model_name: `gpt-${n}`,
            display_name: `gpt-${n}`,
            notes: null,
            context_window_tokens: 128_000,
          },
        });
        if (!profile.ok) return `profile ${JSON.stringify(profile.error)}`;
        revision = profile.value.revision;
      }
      return 'ok';
    },
    { url, count: CODEX_MODELS },
  );
  expect(made).toBe('ok');
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core in TEMP; scripted loopback model only.
test('L10.15 a file named by a link, the thinking level and a model menu with many models', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l10-15-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace, { recursive: true });
  await writeFile(
    join(workspace, 'subway-runner.html'),
    '<!doctype html>\n<title>地铁跑酷</title>\n',
  );
  const bodies: Record<string, unknown>[] = [];
  const model = await writeProvider({
    path: 'unused.md',
    text: '',
    // How a GPT model names the file it wrote: a Markdown link to its absolute path.
    finalText: `已写好：[subway-runner.html](${join(workspace, 'subway-runner.html')})（单文件）。`,
    planAction: (request) => (request.step === 0 ? read : { tool: null, arguments: {} }),
    observeRequest: ({ body }) => bodies.push(body as Record<string, unknown>),
  });
  const { app, page } = await launch(root);
  try {
    await width(app, 1400, 820);
    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L10.15',
      text: '帮我写一个地铁跑酷小游戏',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await page.locator('.session-row').first().click();
    await answered(page, session, 1);

    // Item 4: the link is the file link, and it opens the file.
    const turn = page.locator('.agent-turn').first();
    const link = turn
      .getByRole('paragraph')
      .filter({ hasText: '已写好：' })
      .getByRole('button', { name: '打开 subway-runner.html' });
    await expect(link).toBeVisible({ timeout: 30_000 });
    await expect(turn).not.toContainText('[blocked]');
    await expect(turn).toContainText('已写好：subway-runner.html（单文件）。');
    await turn.screenshot({ path: info.outputPath('file-link.png') });
    await link.click();
    const sidebar = page.getByRole('complementary', { name: '右侧工作区' });
    await expect(sidebar.getByText('<title>地铁跑酷</title>')).toBeVisible({ timeout: 30_000 });

    // Item 2: the thinking level beside the model; the default sends nothing, a level is sent with
    // the model's next request.
    expect(bodies.at(-1)).not.toHaveProperty('reasoning_effort');
    const thinking = page.getByRole('button', { name: '思考程度：默认' });
    await expect(thinking).toBeVisible();
    await thinking.click();
    const levels = page.getByRole('menu', { name: '思考程度' });
    await expect(levels.getByRole('menuitemradio')).toHaveText(['默认', '低', '中', '高']);
    await levels.screenshot({ path: info.outputPath('thinking-levels.png') });
    await levels.getByRole('menuitemradio', { name: '高' }).click();
    await expect(page.getByRole('button', { name: '思考程度：高' })).toBeVisible({
      timeout: 30_000,
    });
    const before = bodies.length;
    // The level is the conversation's own, and goes with what its composer sends.
    const box = page.getByRole('textbox', { name: '输入消息' });
    await box.fill('再写一个走迷宫小游戏');
    await box.press('Enter');
    await answered(page, session, 2);
    expect(bodies.length).toBeGreaterThan(before);
    expect(bodies.at(-1)).toHaveProperty('reasoning_effort', 'high');

    // Item 1: with many models the menu lists the providers and opens one's models beside it;
    // both lists stay inside the window and scroll.
    await manyModels(page, model.url);
    await page.getByTitle(/^模型：/u).click();
    const menu = page.getByRole('menu', { name: '快速选择模型' });
    const providers = menu.getByRole('group', { name: '提供商' });
    await expect(providers.getByRole('menuitem')).toHaveCount(2);
    await providers.getByRole('menuitem', { name: /Codex/u }).hover();
    const codex = page.getByRole('menu', { name: 'Codex 的模型' });
    await expect(codex.getByRole('menuitemradio')).toHaveCount(CODEX_MODELS);
    const viewport = await page.evaluate(() => ({ width: innerWidth, height: innerHeight }));
    for (const list of [menu, codex]) {
      const box = (await list.boundingBox())!;
      expect(box.y).toBeGreaterThanOrEqual(0);
      expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
      expect(box.x).toBeGreaterThanOrEqual(0);
    }
    expect(
      await codex.evaluate((list) => list.scrollHeight > list.clientHeight + 1),
      'fourteen models scroll inside their list',
    ).toBe(true);
    await page.screenshot({ path: info.outputPath('model-menu.png') });
  } finally {
    await app.close();
    await model.close();
  }
});
