import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { finished, launch } from './s06/harness';
import { writeProvider } from './s06-file-write-provider.mjs';

// A workspace of tens of thousands of files is usable at once after it is added: it is in the
// sidebar within a second, a session can be created and its first message sent within three, the
// file tree's root lists within a second, and the "索引中" mark appears and then goes.

const NAME = 'Large workspace';

async function largeTree(root: string) {
  await mkdir(root, { recursive: true });
  await writeFile(join(root, '.gitignore'), 'node_modules/\n');
  for (let module = 0; module < 300; module += 1) {
    const folder = join(root, 'src', `module-${String(module).padStart(3, '0')}`);
    await mkdir(folder, { recursive: true });
    await Promise.all(
      Array.from({ length: 120 }, (_, file) =>
        writeFile(join(folder, `file-${file}.ts`), `export const value${file} = ${file};\n`),
      ),
    );
  }
  // An ignored folder is recorded as itself and never walked.
  for (let pkg = 0; pkg < 50; pkg += 1) {
    const folder = join(root, 'node_modules', `pkg-${pkg}`);
    await mkdir(folder, { recursive: true });
    await Promise.all(
      Array.from({ length: 100 }, (_, file) => writeFile(join(folder, `${file}.js`), '1;\n')),
    );
  }
}

// eslint-disable-next-line no-empty-pattern -- one isolated Electron and Core.
test('a large workspace is usable within three seconds of being added', async ({}, info) => {
  test.setTimeout(900_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l12-1-large-'));
  const workspaceRoot = join(root, 'workspace');
  await largeTree(workspaceRoot);
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const provider = await writeProvider({
    path: 'unused.txt',
    text: '',
    planAction: () => ({ tool: null, arguments: {} }),
    finalText: '好的。',
  });
  const { app, page } = await launch(root);
  try {
    // The model is configured beforehand: the clock below is the add and what follows it.
    const profileId = await page.evaluate(async (url) => {
      const api = window.aura.v1;
      const settings = await api.getModelSettings();
      if (!settings.ok) throw new Error('model settings');
      const provider = await api.createModelProvider({
        expected_revision: settings.value.models_revision,
        input: {
          display_name: 'Large workspace model',
          source: { type: 'custom', catalog_provider_id: null },
        },
      });
      if (!provider.ok) throw new Error(JSON.stringify(provider));
      const endpoint = await api.createModelEndpoint({
        expected_revision: provider.value.revision,
        provider_id: provider.value.value.id,
        display_name: 'Local endpoint',
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
      if (!endpoint.ok) throw new Error(JSON.stringify(endpoint));
      const profile = await api.createModelProfile({
        expected_revision: endpoint.value.revision,
        input: {
          endpoint_id: endpoint.value.value.id,
          provider_model_name: 'fixture',
          display_name: 'Large workspace model',
          notes: null,
          context_window_tokens: 256000,
        },
      });
      if (!profile.ok) throw new Error(JSON.stringify(profile));
      const enabled = await api.updateModelProfile({
        expected_revision: profile.value.revision,
        model_profile_id: profile.value.value.id,
        display_name: 'Large workspace model',
        notes: null,
        enabled: true,
        request_defaults: {
          max_output_tokens: 4096,
          temperature: null,
          top_p: null,
          chat_output_token_field: null,
          chat_instruction_role: null,
        },
        historical_snapshot_max_tokens: 1024,
      });
      if (!enabled.ok) throw new Error(JSON.stringify(enabled));
      // The default model, which a new conversation's composer starts with.
      const current = await api.getModelSettings();
      if (!current.ok) throw new Error('model settings');
      const chosen = await api.setDefaultModel({
        expected_revision: current.value.aura_revision,
        model_profile_id: profile.value.value.id,
      });
      if (!chosen.ok) throw new Error(JSON.stringify(chosen));
      return profile.value.value.id;
    }, provider.url);

    const started = Date.now();
    const added = await page.evaluate(
      async ({ selectedRoot, name }) => {
        const workspace = await window.aura.v1.createWorkspace({
          display_name: name,
          selected_root: selectedRoot,
        });
        if (!workspace.ok) throw new Error(JSON.stringify(workspace));
        return workspace.value.workspace.workspace_id;
      },
      { selectedRoot: workspaceRoot, name: NAME },
    );
    const addedMs = Date.now() - started;
    const row = page.locator('.workspace-select', { hasText: NAME });
    await expect(row).toBeVisible({ timeout: 5_000 });
    const sidebarMs = Date.now() - started;
    const badge = row.locator('.workspace-index-badge');
    await expect(badge).toHaveText('索引中', { timeout: 2_000 });
    const markSeen = true;

    const first = await page.evaluate(
      async ({ workspaceId, model }) => {
        const api = window.aura.v1;
        const session = await api.createSession({ workspace_id: workspaceId, title: '' });
        if (!session.ok) throw new Error(JSON.stringify(session));
        const sessionId = session.value.session_id;
        const binding = await api.setSessionModelBinding({
          session_id: sessionId,
          expected_revision: '0',
          model_profile_id: model,
        });
        if (!binding.ok) throw new Error(JSON.stringify(binding));
        const submitted = await api.submitAgentMessage({
          session_id: sessionId,
          destination: { lane: 'next_turn' },
          text: '你好',
        });
        if (!submitted.ok) throw new Error(JSON.stringify(submitted));
        return sessionId;
      },
      { workspaceId: added, model: profileId },
    );
    const firstMessageMs = Date.now() - started;

    // The same through the window, as the user does it while the index is still being built: a
    // new conversation from the sidebar, 你好, send — the message is in the conversation at once.
    await page.getByRole('button', { name: `在 ${NAME} 新建会话` }).click();
    await page.getByRole('textbox', { name: '输入消息' }).fill('你好');
    const sent = Date.now();
    await page.getByRole('button', { name: '发送消息' }).click();
    await expect(page.locator('.agent-user-message', { hasText: '你好' }).first()).toBeVisible({
      timeout: 5_000,
    });
    const shownMs = Date.now() - sent;

    const treeStarted = Date.now();
    const rootListing = await page.evaluate(
      async ({ workspaceId, sessionId }) => {
        const listed = await window.aura.v1.listWorkspaceFiles({
          workspace_id: workspaceId,
          session_id: sessionId,
          target: { path: '.' },
        });
        if (!listed.ok) throw new Error(JSON.stringify(listed));
        return listed.value.entries.map((entry) => entry.name);
      },
      { workspaceId: added, sessionId: first },
    );
    const treeMs = Date.now() - treeStarted;
    expect(rootListing).toEqual(expect.arrayContaining(['.gitignore', 'node_modules', 'src']));

    // The mark goes when the index is built, and the first message really runs.
    await expect(badge).toBeHidden({ timeout: 300_000 });
    const indexedMs = Date.now() - started;

    const timings = { addedMs, sidebarMs, firstMessageMs, shownMs, treeMs, indexedMs, markSeen };
    await info.attach('timings', {
      body: JSON.stringify(timings, null, 2),
      contentType: 'application/json',
    });
    console.log(`L12_1_LARGE_WORKSPACE ${JSON.stringify(timings)}`);
    expect(sidebarMs, 'in the sidebar within a second').toBeLessThanOrEqual(1_000);
    expect(firstMessageMs, 'a first message within three seconds').toBeLessThanOrEqual(3_000);
    expect(treeMs, 'the tree root within a second').toBeLessThanOrEqual(1_000);
    expect(shownMs, 'a message sent from the window shows within a second').toBeLessThanOrEqual(
      1_000,
    );
    await finished(page, first);
  } finally {
    await app.close();
    await provider.close();
  }
});
