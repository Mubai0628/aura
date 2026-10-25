import { _electron as electron, expect, test, type Page } from '@playwright/test';
import { createServer } from 'node:http';
import { copyFile, mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { finished, pending } from './s06/harness';

/**
 * A screenshot tour of the real app for design review: startup, a conversation with an
 * approval, the right-hand tools, a new session and every Settings page, at two window sizes.
 * It checks nothing about behaviour and is skipped unless AURA_VISUAL_TOUR_OUT names a folder.
 */
const OUT = process.env.AURA_VISUAL_TOUR_OUT;
test.skip(!OUT, 'visual tour: set AURA_VISUAL_TOUR_OUT to a folder for the screenshots');

const wait = (ms: number) => new Promise((done) => setTimeout(done, ms));

const PLAN = `我先看了一下项目结构，接下来写一份简短的说明：

## 计划

1. 读 \`README.md\` 与 \`src/\` 的入口
2. 把**模块职责**整理成一张表
3. 写入 \`NOTES.md\`

\`\`\`rust
fn main() {
    println!("hello, aura");
}
\`\`\``;

const DONE = `已写好 \`NOTES.md\`。要点：

| 模块 | 职责 |
| --- | --- |
| \`src/main.rs\` | 程序入口 |
| \`README.md\` | 使用说明 |

> 只读会话里写文件会先问你；这次你允许了本轮写入。

- 下一步可以补上测试
- 或者把说明并入 README`;

/** One OpenAI-compatible stream per request: a plan and an `fs.write`, then the answer. */
async function tourProvider() {
  const steps = new Map<string, number>();
  const server = createServer((request, response) => {
    let raw = '';
    request.on('data', (part) => (raw += part));
    request.on('end', () => void respond(JSON.parse(raw)));
    async function respond(body: {
      tools?: Array<{ function: { name: string } }>;
      messages: Array<{ role: string; content: string }>;
    }) {
      response.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
      const send = (delta: object, finish: string | null = null) =>
        response.write(
          `data: ${JSON.stringify({
            id: 'tour',
            choices: [{ index: 0, delta, finish_reason: finish }],
            ...(finish
              ? { usage: { prompt_tokens: 18000, completion_tokens: 900, total_tokens: 18900 } }
              : {}),
          })}\n\n`,
        );
      // Memory upkeep asks without tools: answer with nothing to keep.
      if (!body.tools?.length) {
        send(
          { role: 'assistant', content: '{"schema_version":1,"candidates":[],"events":[]}' },
          'stop',
        );
        response.end('data: [DONE]\n\n');
        return;
      }
      const user = body.messages.filter((message) => message.role === 'user').length;
      const key = String(user);
      const step = steps.get(key) ?? 0;
      steps.set(key, step + 1);
      send({ role: 'assistant' });
      const text = step === 0 ? PLAN : DONE;
      for (let at = 0; at < text.length; at += 24) {
        send({ content: text.slice(at, at + 24) });
        await wait(15);
      }
      const write = body.tools.find((tool) => tool.function.name.startsWith('fs_write__'));
      if (step === 0 && write) {
        const call = {
          index: 0,
          id: 'tour_call_0',
          type: 'function',
          function: {
            name: write.function.name,
            arguments: JSON.stringify({
              path: 'NOTES.md',
              text: '# 说明\n\n- `src/main.rs`：程序入口\n',
              expected: { expected_kind: 'absent' },
              aura_purpose: '写入项目说明',
            }),
          },
        };
        send({ tool_calls: [call] }, 'tool_calls');
      } else {
        send({}, 'stop');
      }
      response.end('data: [DONE]\n\n');
    }
  });
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
  const address = server.address() as { port: number };
  return {
    url: `http://127.0.0.1:${address.port}/v1`,
    close: () => new Promise<void>((done) => server.close(() => done())),
  };
}

/** A plugin `kit` with a Skill, a prompt command and the repository's test MCP server. */
async function pluginFolder(root: string): Promise<string> {
  const plugin = join(root, 'source', 'kit');
  await mkdir(join(plugin, 'bin'), { recursive: true });
  await mkdir(join(plugin, 'skills', 'notes'), { recursive: true });
  await mkdir(join(plugin, 'commands', 'summarize'), { recursive: true });
  await writeFile(
    join(plugin, 'aura-plugin.toml'),
    'schema_version = 1\nname = "kit"\nversion = "1.2.0"\ndescription = "代码审查用的 Skill、命令与服务器"\nauthor = "Aura"\n\n[limits]\nmemory_mb = 512\n',
  );
  await copyFile(
    resolve(import.meta.dirname, '../../../target/debug/aura-mcp-fixture.exe'),
    join(plugin, 'bin', 'server.exe'),
  );
  await writeFile(
    join(plugin, 'mcp.json'),
    JSON.stringify({
      servers: [
        {
          name: 'files',
          display_name: '插件文件',
          transport: { type: 'stdio', command: '${PLUGIN_ROOT}/bin/server.exe', arguments: [] },
        },
      ],
    }),
  );
  await writeFile(
    join(plugin, 'skills', 'notes', 'SKILL.md'),
    '---\nname: notes\ndescription: 用三条要点总结\n---\n\nAnswer in three bullets.\n',
  );
  await writeFile(
    join(plugin, 'commands', 'summarize', 'command.toml'),
    'name = "summarize"\ntitle = "总结改动"\ndescription = "总结本次改动"\nversion = 1\n',
  );
  await writeFile(
    join(plugin, 'commands', 'summarize', 'template.md'),
    '总结 $ARGUMENTS 的改动。\n',
  );
  return plugin;
}

async function shot(page: Page, name: string) {
  await page.screenshot({ path: join(OUT as string, `${name}.png`) });
}

/** The rest of a long Settings page: its content pane scrolled a screen at a time. */
async function scrolled(page: Page, name: string, parts = 5) {
  for (let part = 2; part <= parts; part += 1) {
    const moved = await page.evaluate(() => {
      const pane = document.querySelector('.settings-content');
      if (!pane || pane.scrollTop + pane.clientHeight >= pane.scrollHeight - 4) return false;
      pane.scrollTop += pane.clientHeight - 80;
      return true;
    });
    if (!moved) return;
    await wait(250);
    await shot(page, `${name}-${part}`);
  }
}

/** Takes a screenshot after `step`; a step that fails is noted and the tour goes on. */
async function capture(page: Page, name: string, step: () => Promise<unknown>) {
  try {
    await step();
    await wait(450);
    await shot(page, name);
  } catch (error) {
    await writeFile(join(OUT as string, `${name}.error.txt`), String(error));
  }
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core with TEMP data; scripted loopback model only.
test('visual tour of the desktop app', async ({}, info) => {
  test.setTimeout(240_000);
  await mkdir(OUT as string, { recursive: true });
  const root = await mkdtemp(join(tmpdir(), 'aura-visual-tour-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(join(workspace, 'src'), { recursive: true });
  await writeFile(join(workspace, 'README.md'), '# Demo\n\nA small project for the tour.\n');
  await writeFile(join(workspace, 'src', 'main.rs'), 'fn main() {\n    println!("hello");\n}\n');
  const model = await tourProvider();
  const app = await electron.launch({
    args: [`--user-data-dir=${join(root, 'Electron')}`, resolve(import.meta.dirname, '..')],
    env: {
      ...process.env,
      APPDATA: join(root, 'Roaming'),
      LOCALAPPDATA: join(root, 'Local'),
      AURA_MEMORY_TEST_ROOT: join(root, 'global-memories'),
      AURA_USER_PROFILE_TEST_ROOT: join(root, 'user-profile'),
      AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
      AURA_E2E: '1',
      AURA_E2E_CORE_MODE: 'normal',
      AURA_P05_CLEAN_ACCEPTANCE: '1',
      // The terminal tab opens a terminal at once; never with the owner's profile.
      AURA_TERMINAL_TEST_NO_PROFILE: '1',
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
  });
  try {
    const page = await app.firstWindow();
    // The window the page lives in: the first window in Electron's list is not always that one,
    // and a resize that silently missed it left a "narrow" capture at full width.
    const size = async (width: number, height: number) => {
      await app.evaluate(
        ({ BrowserWindow }, [w, h]) => {
          for (const window of BrowserWindow.getAllWindows())
            if (!window.isDestroyed()) window.setSize(w!, h!);
        },
        [width, height],
      );
      // The page is inside the window frame: a little narrower than the window, never wider.
      await expect
        .poll(async () => {
          const inner = await page.evaluate(() => window.innerWidth);
          return inner <= width && inner > width - 60;
        })
        .toBe(true);
    };
    await size(1440, 900);
    await shot(page, '00-startup-first');
    let elapsed = 0;
    for (const at of [150, 400, 800, 1400]) {
      await wait(at - elapsed);
      elapsed = at;
      await shot(page, `01-startup-${at}ms`);
    }
    await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 60_000 });
    await capture(page, '02-ready-empty', async () => undefined);

    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: '演示项目',
      text: '帮我看看这个项目，并写一份简短的说明',
    });
    const session = setup.sessionId as string;
    await capture(page, '03-approval', async () => {
      await pending(page, session);
      await page.locator('.session-row').first().click();
      await expect(page.getByRole('region', { name: '当前任务审批' })).toBeVisible();
    });
    // The card shows the model's explanation; the rest is under 查看详情.
    await capture(page, '03b-approval-details', () =>
      page.getByRole('region', { name: '当前任务审批' }).getByText('查看详情').click(),
    );
    await capture(page, '04-conversation', async () => {
      await page
        .getByRole('region', { name: '当前任务审批' })
        .getByRole('button', { name: /^(本轮允许|仅此一次)$/u })
        .first()
        .click();
      await finished(page, session);
    });
    // A finished turn's sections are closed; open the first to see its rows. The
    // finished turn folds its process first; its status line opens it.
    await capture(page, '04b-process-open', async () => {
      await page.locator('.agent-turn-toggle').last().click();
      await page.locator('.agent-process-section > button').first().click();
    });
    // The context usage after real requests — a slim bar of moving fluid; a part's name
    // and amount float above it where the pointer is.
    await capture(page, '04c-context-card', () =>
      page.getByRole('button', { name: /^上下文占用/u }).click(),
    );
    await capture(page, '04c2-context-part', async () => {
      const track = page.locator('.context-bar-track');
      const box = await track.boundingBox();
      if (box) await page.mouse.move(box.x + 3, box.y + box.height / 2);
      await page.getByRole('tooltip').waitFor({ timeout: 5_000 });
    });
    await page.keyboard.press('Escape');
    // A path in the reply has its own right-click menu.
    await capture(page, '04d-link-menu', async () => {
      await page.locator('.markdown-file-link').first().click({ button: 'right' });
      await page.getByRole('menu').waitFor({ timeout: 5_000 });
    });
    await page.keyboard.press('Escape');
    // A fenced block keeps its lines: the three lines of the plan's Rust block are three rows.
    const rows = await page
      .locator('.markdown-body [data-streamdown="code-block"] code > span')
      .evaluateAll((lines) => lines.map((line) => Math.round(line.getBoundingClientRect().top)));
    expect(rows).toHaveLength(3);
    expect(new Set(rows).size, `rows at ${rows.join(', ')}`).toBe(3);
    await capture(page, '05-tool-picker', () =>
      page.getByRole('button', { name: '打开工具' }).click(),
    );
    for (const [name, label] of [
      ['06-files', '文件'],
      ['07-terminal', '终端'],
      ['08-review', '审查'],
      ['09-process', '进程'],
    ] as const) {
      await capture(page, name, async () => {
        const picker = page.getByRole('menu', { name: '选择工具' });
        if (!(await picker.isVisible()))
          await page.getByRole('button', { name: '打开工具' }).click();
        await picker.getByRole('menuitem', { name: new RegExp(label, 'u') }).click();
      });
    }
    // A file open in the 文件 tab, its tree beside it.
    await capture(page, '09a-file-open', async () => {
      await page.getByRole('tab', { name: '文件', exact: true }).click();
      await page
        .getByRole('navigation', { name: '文件树' })
        .getByRole('button', { name: 'README.md', exact: true })
        .click();
      await page.getByRole('heading', { level: 1, name: 'Demo' }).waitFor();
    });
    // A file row's right-click menu, and a tab's.
    await capture(page, '09a2-file-menu', async () => {
      await page
        .getByRole('navigation', { name: '文件树' })
        .getByRole('button', { name: 'README.md', exact: true })
        .click({ button: 'right' });
      await page.getByRole('menu').waitFor({ timeout: 5_000 });
    });
    await page.keyboard.press('Escape');
    await capture(page, '09a3-tab-menu', async () => {
      await page
        .getByRole('tablist', { name: '已打开的工具' })
        .getByRole('tab', { name: '文件', exact: true })
        .click({ button: 'right' });
      await page.getByRole('menu').waitFor({ timeout: 5_000 });
    });
    await page.keyboard.press('Escape');
    // A file open in the 文件 page is a small tab of its own there, with the file's menu.
    await capture(page, '09a4-file-tab-menu', async () => {
      await page
        .getByRole('tablist', { name: '打开的文件' })
        .getByRole('tab', { name: 'README.md' })
        .click({ button: 'right' });
      await page.getByRole('menu').waitFor({ timeout: 5_000 });
    });
    await page.keyboard.press('Escape');
    // The conversation menu. It archives; restoring and deleting are
    // in 设置 → 归档, where deleting asks first.
    await capture(page, '09b-session-menu', () =>
      page
        .locator('.session-nav-item')
        .first()
        .getByRole('button', { name: /更多操作$/u })
        .click(),
    );
    await page.keyboard.press('Escape');
    // A conversation made and archived for the question, so the tour's own stays as it is.
    await page.evaluate(async (workspace_id) => {
      const api = window.aura.v1;
      const made = await api.createSession({ workspace_id, title: '归档的演示会话' });
      if (made.ok) await api.archiveSession({ session_id: made.value.session_id });
    }, setup.workspaceId as string);
    await capture(page, '09c-delete-confirm', async () => {
      await page.getByRole('button', { name: '设置', exact: true }).click();
      await page
        .locator('.settings-navigation')
        .getByRole('button', { name: '归档', exact: true })
        .click();
      await page
        .getByRole('region', { name: '已归档的会话' })
        .getByRole('button', { name: '删除' })
        .click();
    });
    // The irreversible action reads as such: its button is the danger color, not the default.
    const destructive = page.getByRole('dialog').getByRole('button', { name: '删除会话' });
    const danger = await page.evaluate(() =>
      getComputedStyle(document.documentElement).getPropertyValue('--danger').trim(),
    );
    const fill = await destructive.evaluate((element) => getComputedStyle(element).backgroundColor);
    expect(fill).toBe(
      await page.evaluate((color) => {
        const probe = document.createElement('span');
        probe.style.color = color;
        document.body.append(probe);
        const value = getComputedStyle(probe).color;
        probe.remove();
        return value;
      }, danger),
    );
    await page.getByRole('dialog').getByRole('button', { name: '取消' }).click();
    // Settings open where they were left: back to 模型 for the tour's later visit.
    await page
      .locator('.settings-navigation')
      .getByRole('button', { name: '模型', exact: true })
      .click();
    await page.getByRole('button', { name: '返回会话' }).click();
    await capture(page, '10-new-session', () =>
      page.getByRole('button', { name: '在 演示项目 新建会话' }).click(),
    );
    // A trusted plugin, so the extension pages show cards rather than empty states.
    const source = await pluginFolder(root);
    await page.evaluate(async (path) => {
      const api = window.aura.v1;
      const listed = await api.listPlugins({});
      if (!listed.ok) return;
      const imported = await api.importPlugin({ path, expected_revision: listed.value.revision });
      if (!imported.ok) return;
      const shown = await api.previewPlugin({ name: 'kit', pending: false });
      if (!shown.ok) return;
      await api.trustPlugin({
        name: 'kit',
        pending: false,
        expected_fingerprint: shown.value.fingerprint ?? '',
        expected_revision: imported.value.revision,
      });
    }, source);
    await capture(page, '11-settings-models', () =>
      page.getByRole('button', { name: '设置', exact: true }).click(),
    );
    await scrolled(page, '11-settings-models');
    await capture(page, '11-settings-provider', () =>
      page
        .getByRole('button', { name: /^配置 /u })
        .first()
        .click(),
    );
    await page.getByRole('button', { name: '关闭 Provider 配置' }).click();
    for (const [value, label] of [
      ['tools', '工具'],
      ['environment', '环境'],
      ['security', '权限与沙箱'],
      ['memory', '记忆'],
      ['commands', '命令'],
      ['plugins', '插件'],
      ['skills', 'Skill'],
      ['mcp', 'MCP'],
      ['hooks', 'Hook'],
      ['archive', '归档'],
    ] as const) {
      await capture(page, `12-settings-${value}`, () =>
        page
          .locator('.settings-navigation')
          .getByRole('button', { name: label, exact: true })
          .click(),
      );
      await scrolled(page, `12-settings-${value}`, value === 'tools' ? 16 : 5);
    }
    // The MCP form with the OAuth sign-in chosen.
    await capture(page, '12-settings-mcp-oauth', async () => {
      await page
        .locator('.settings-navigation')
        .getByRole('button', { name: 'MCP', exact: true })
        .click();
      await page.getByRole('button', { name: '添加服务器' }).click();
      const form = page.getByRole('form', { name: '添加 MCP 服务器' });
      await form.getByLabel('HTTP 地址（Streamable HTTP）').check();
      await form.getByLabel('OAuth 登录（在浏览器中登录）').check();
      await form.getByLabel('OAuth 登录（在浏览器中登录）').scrollIntoViewIfNeeded();
    });
    await capture(page, '13-narrow-settings', async () => {
      await size(980, 680);
    });
    await capture(page, '14-narrow-session', async () => {
      await page.getByRole('button', { name: '返回会话' }).click();
      await page.locator('.session-row').first().click();
    });
    // With the right panel open the composer is narrow, and its row of controls
    // still stays on one line.
    expect(await page.evaluate(() => window.innerWidth)).toBeLessThan(1000);
    await capture(page, '14b-narrow-session-panel', async () => {
      const reopen = page.getByRole('button', { name: '展开右侧边栏' });
      if ((await reopen.count()) > 0) await reopen.click();
    });
    await expect(page.locator('#right-workspace')).toBeVisible();
    const box = async (name: string) => {
      const found = await page.getByRole('button', { name, exact: true }).boundingBox();
      expect(found, name).not.toBeNull();
      return found!;
    };
    const plus = await box('添加');
    const send = await box('发送消息');
    expect(Math.abs(plus.y + plus.height / 2 - (send.y + send.height / 2))).toBeLessThan(3);
    // …and its controls do not run into each other: the model name gives way first.
    const permission = await page
      .locator('.composer-controls-leading .composer-select')
      .boundingBox();
    const ring = await page
      .locator('.composer-controls-trailing .context-ring-button')
      .boundingBox();
    expect(permission!.x + permission!.width).toBeLessThanOrEqual(ring!.x + 1);
  } finally {
    await app.close();
    await model.close();
  }
});
