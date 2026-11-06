import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { finished, launch } from './s06/harness';
import { writeProvider } from './s06-file-write-provider.mjs';
import { expectSecureSurface } from './electron-shell/renderer-api';
import { addAndTrust, panelText, scriptedDefaults, workspace } from './l17/setup';

// A panel with its plugin's own parts: a prompt command, a text setting, and the conversation's
// events, as the Skill's panel reference describes them.
const MANIFEST = `schema_version = 2
name = "own-panel"
version = "0.1.0"
description = "自己的命令和设置"
capabilities = ["session.events", "commands", "settings"]

[panel]
title = "自己的"

[[settings]]
key = "greeting"
title = "问候"
`;
const COMMAND = `name = "summary"
description = "总结改动"
argument_hint = "<范围>"
version = 1
`;
const PAGE = `<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8" />
    <title>自己的</title>
  </head>
  <body>
    <pre id="shown"></pre>
    <script src="main.js"></script>
  </body>
</html>
`;
// What each call gave, shown as JSON for the test to read.
const SCRIPT = `const shown = {};
const show = () => {
  document.getElementById('shown').textContent = JSON.stringify(shown);
};
async function main() {
  shown.calls = Object.keys(auraPanel).sort();
  shown.before = await auraPanel.settings();
  shown.set = await auraPanel.setSetting('greeting', ' 你好 ');
  shown.read = await auraPanel.settings();
  shown.refused = await auraPanel.setSetting('ghost', 'x').catch((error) => error.message);
  let { cursor } = await auraPanel.events();
  shown.events = [];
  shown.ran = await auraPanel.run('summary', { arguments: 'src/lib.rs', dedupeKey: 'run-1' });
  shown.again = await auraPanel.run('summary', { arguments: 'src/lib.rs', dedupeKey: 'run-1' });
  show();
  setInterval(async () => {
    const read = await auraPanel.events(cursor);
    cursor = read.cursor;
    shown.events.push(...read.events.map((each) => each.event));
    show();
  }, 1000);
}
main().catch((error) => {
  shown.error = String(error);
  show();
});
`;

async function ownPlugin(root: string) {
  const folder = join(root, 'plugins', 'own-panel');
  await mkdir(join(folder, 'panel'), { recursive: true });
  await mkdir(join(folder, 'commands', 'summary'), { recursive: true });
  await writeFile(join(folder, 'aura-plugin.toml'), MANIFEST);
  await writeFile(join(folder, 'panel', 'index.html'), PAGE);
  await writeFile(join(folder, 'panel', 'main.js'), SCRIPT);
  await writeFile(join(folder, 'commands', 'summary', 'command.toml'), COMMAND);
  await writeFile(
    join(folder, 'commands', 'summary', 'template.md'),
    '总结一下 $ARGUMENTS 的改动\n',
  );
  return folder;
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core and the plugin's panel page, a TEMP workspace and isolated settings; scripted loopback model only.
test("L17 a panel runs its plugin's own command, reads and sets its own setting and is told of the steps", async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l17-panel-own-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const work = join(root, 'work');
  await mkdir(work);
  const model = await writeProvider({
    path: 'unused.txt',
    text: '',
    planAction: () => ({ tool: null, arguments: {} }),
  });
  const { app, page } = await launch(root);
  try {
    await expectSecureSurface(page);
    const profile = await scriptedDefaults(page, model.url, 'workspace_read_write');
    const space = await workspace(page, work, 'Own');
    await addAndTrust(app, page, await ownPlugin(root), 'own-panel');
    const session = await page.evaluate(
      async ({ space, profile }) => {
        const api = window.aura.v1;
        const created = await api.createSession({ workspace_id: space, title: '' });
        if (!created.ok) return JSON.stringify(created);
        const bound = await api.setSessionModelBinding({
          session_id: created.value.session_id,
          expected_revision: '0',
          model_profile_id: profile,
        });
        return bound.ok ? created.value.session_id : JSON.stringify(bound);
      },
      { space, profile },
    );
    expect(session).toMatch(/^ses_/u);

    // Opened in the conversation, the page has only the calls its plugin declared.
    await page.getByRole('button', { name: '返回会话' }).click();
    const open = page.getByRole('button', { name: '展开 Own', exact: true });
    if (await open.isVisible()) await open.click();
    await page.locator('.session-row').first().click();
    await page.getByRole('button', { name: '打开工具' }).click();
    await page
      .getByRole('menu', { name: '选择工具' })
      .getByRole('menuitem', { name: /自己的/u })
      .click();
    const shown = async () => {
      const text = await panelText(app, 'own-panel');
      return text === '' ? {} : (JSON.parse(text) as Record<string, unknown>);
    };
    const kinds = async () =>
      ((await shown()).events as { kind: string }[] | undefined)?.map((event) => event.kind);
    await expect
      .poll(kinds, { timeout: 60_000 })
      .toEqual(['turn_started', 'step_started', 'step_ended', 'turn_ended']);
    const seen = await shown();
    expect(seen.error).toBeUndefined();
    expect(seen.calls).toEqual([
      'capabilities',
      'events',
      'run',
      'setSetting',
      'settings',
      'version',
    ]);
    // Its setting, set as the plugin page sets it; a key its manifest does not declare is refused.
    expect(seen.before).toEqual({ values: {} });
    expect(seen.set).toEqual({ values: { greeting: '你好' } });
    expect(seen.read).toEqual({ values: { greeting: '你好' } });
    expect(String(seen.refused)).toMatch(/^plugins\.setting_unknown/u);
    // Its command ran once as its message; the same key again was the same message.
    expect(seen.ran).toMatchObject({ repeated: false });
    expect(seen.again).toMatchObject({ repeated: true });
    const events = seen.events as Record<string, unknown>[];
    expect(events[2]).toMatchObject({
      kind: 'step_ended',
      usage: { input_tokens: 1000, cached_input_tokens: 0, output_tokens: 100 },
    });
    await finished(page, session);

    // In the conversation, the command's message is the plugin's; the plugin page keeps the value.
    const turn = page.locator('.agent-turn').filter({ hasText: '总结一下 src/lib.rs 的改动' });
    await expect(turn).toHaveCount(1);
    await expect(turn.getByText('来自插件 own-panel')).toBeVisible();
    const kept = await page.evaluate(async () => {
      const listed = await window.aura.v1.listPlugins({});
      if (!listed.ok) return JSON.stringify(listed);
      const plugin = listed.value.plugins.find((each) => each.name === 'own-panel');
      return plugin?.options?.find((each) => each.option.key === 'greeting')?.text;
    });
    expect(kept).toBe('你好');
    expect(model.requests).toHaveLength(1);
    expect(model.errors).toEqual([]);
  } finally {
    await app.close();
    await model.close();
  }
});
