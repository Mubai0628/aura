import { expect, test, type Page } from '@playwright/test';
import { mkdir, mkdtemp, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { launch } from './s06/harness';

// The conversation's scrollbar is not covered by the
// composer's dock; many sidebar tabs scroll and close from a menu; Main opens nothing it should not;
// the context tube is painted by WebGL in the real window.

async function workspace() {
  const root = await mkdtemp(join(tmpdir(), 'aura-l10-9-'));
  const project = join(root, 'workspace');
  const outside = join(root, 'outside');
  await mkdir(join(project, 'scripts'), { recursive: true });
  await mkdir(outside);
  await writeFile(join(project, 'scripts', 'build.bat'), '@echo off\r\necho build\r\n');
  await writeFile(join(project, 'README.md'), '# Demo\n');
  await writeFile(join(outside, 'secret.txt'), 'outside the workspace');
  // A junction needs no privilege: a folder in the workspace that leads out of it.
  await symlink(outside, join(project, 'escape'), 'junction');
  return { root, project };
}

async function session(page: Page, project: string) {
  const ids = await page.evaluate(async (selected_root) => {
    const created = await window.aura.v1.createWorkspace({
      display_name: 'L10.9 workspace',
      selected_root,
    });
    if (!created.ok) throw Error(created.error.message);
    const workspace_id = created.value.workspace.workspace_id;
    const opened = await window.aura.v1.createSession({ workspace_id, title: 'L10.9 场景' });
    if (!opened.ok) throw Error(opened.error.message);
    return { workspace: workspace_id, session: opened.value.session_id };
  }, project);
  await page.locator('.session-row').filter({ hasText: 'L10.9 场景' }).click();
  await expect(page.getByRole('heading', { name: 'L10.9 场景', exact: true })).toBeVisible();
  return ids;
}

/** Opens a tool from "+", showing the right sidebar first if a narrow window hid it. */
async function openTool(page: Page, name: RegExp) {
  const tools = page.getByRole('button', { name: '打开工具', exact: true });
  const reopen = page.getByRole('button', { name: '展开右侧边栏', exact: true });
  await expect(tools.or(reopen)).toBeVisible();
  if (await reopen.isVisible()) await reopen.click();
  await tools.click();
  await page.getByRole('menu', { name: '选择工具' }).getByRole('menuitem', { name }).click();
}

test('the scrollbar, many tabs, what Main refuses and the painted context tube', async () => {
  test.setTimeout(180_000);
  const { root, project } = await workspace();
  const { app, page } = await launch(root);
  try {
    const ids = await session(page, project);

    // The conversation's scrollbar beside the composer takes the mouse, not the dock over it.
    const hit = await page.evaluate(() => {
      const scroll = document.querySelector('.conversation-scroll')!.getBoundingClientRect();
      const composer = document.querySelector('.composer')!.getBoundingClientRect();
      const element = document.elementFromPoint(scroll.right - 3, composer.top + 12);
      return {
        dock: Boolean(element?.closest('.conversation-dock')),
        conversation: Boolean(element?.closest('.conversation-scroll')),
      };
    });
    expect(hit).toEqual({ dock: false, conversation: true });

    // Three tabs fit the sidebar as it first opens: no 全部标签, and every name shows whole.
    for (let index = 0; index < 3; index += 1) await openTool(page, /^文件/u);
    const strip = page.getByRole('tablist', { name: '已打开的工具' });
    await expect(strip.getByRole('tab')).toHaveCount(3);
    const three = await strip.evaluate((row) => ({
      frame: row.parentElement!.clientWidth,
      tabs: [...row.querySelectorAll<HTMLElement>('.tool-tab')].map((item) => item.offsetWidth),
      clipped: [...row.querySelectorAll<HTMLElement>('[role="tab"] > span')]
        .filter((label) => label.scrollWidth > label.clientWidth)
        .map((label) => label.textContent),
    }));
    expect(three.clipped, JSON.stringify(three)).toEqual([]);
    await expect(page.getByRole('button', { name: '全部标签' })).toHaveCount(0);

    // Many tabs: past that the row scrolls under the wheel and 全部标签 lists them.
    for (let index = 3; index < 10; index += 1) await openTool(page, /^文件/u);
    await expect(strip.getByRole('tab')).toHaveCount(10);
    const sizes = await strip.evaluate((row) => ({
      inside: row.scrollWidth,
      shown: row.clientWidth,
    }));
    expect(sizes.inside).toBeGreaterThan(sizes.shown);
    await expect(page.getByRole('button', { name: '全部标签' })).toBeVisible();
    // The tab shown (the last opened) is in view.
    const shownTab = await strip.evaluate((row) => {
      const active = row.querySelector('.tool-tab.is-active')!.getBoundingClientRect();
      const frame = row.getBoundingClientRect();
      return active.left >= frame.left - 1 && active.right <= frame.right + 1;
    });
    expect(shownTab).toBe(true);
    await strip.evaluate((row) => {
      row.scrollLeft = 0;
    });
    await strip.hover();
    await page.mouse.wheel(0, 300);
    await expect.poll(() => strip.evaluate((row) => row.scrollLeft)).toBeGreaterThan(0);
    await page.getByRole('button', { name: '全部标签' }).click();
    await expect(page.getByRole('menu', { name: '全部标签' }).getByRole('menuitem')).toHaveCount(
      10,
    );
    await page.keyboard.press('Escape');
    // A tab's menu closes the others.
    await strip.getByRole('tab').first().click({ button: 'right' });
    await page.getByRole('menuitem', { name: '关闭其他标签' }).click();
    await expect(strip.getByRole('tab')).toHaveCount(1);
    await expect(page.getByRole('button', { name: '全部标签' })).toHaveCount(0);

    // Main opens nothing it should not: a script, a path out of the workspace, a junction out
    // of it, an address that is not a web page. Nothing here starts a program.
    const refused = await page.evaluate(async ({ workspace, session }) => {
      const open = (path: string, opener: 'default_application' | 'file_manager') =>
        window.aura.v1.openWorkspaceFileOutside({
          workspace_id: workspace,
          session_id: session,
          path,
          opener,
        });
      return {
        script: await open('scripts/build.bat', 'default_application'),
        up: await open('../outside/secret.txt', 'file_manager'),
        junction: await open('escape/secret.txt', 'file_manager'),
        link: await window.aura.v1.openExternalLink({ url: 'javascript:alert(1)' }),
      };
    }, ids);
    expect(refused.script).toEqual({
      ok: true,
      value: {
        completed: false,
        message: '这是可执行文件或脚本，Aura 不会直接运行它；可以在文件资源管理器中查看。',
      },
    });
    expect(refused.up).toMatchObject({ ok: true, value: { completed: false } });
    expect(refused.junction).toEqual({
      ok: true,
      value: { completed: false, message: '这个文件在工作区外，Aura 不会打开工作区外的文件。' },
    });
    expect(refused.link).toMatchObject({ ok: true, value: { completed: false } });

    // The context card opens from its ring in the real window.
    await page.getByRole('button', { name: /^上下文占用/u }).click();
    const card = page.getByRole('region', { name: '上下文占用详情' });
    await expect(card.locator('.context-pixel-head')).toContainText('上下文');
  } finally {
    await app.close();
  }
});
