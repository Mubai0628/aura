import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { isAbsolute, join, relative, resolve, toNamespacedPath } from 'node:path';
import { closeAura, launchAuraWithDataRoot } from './electron-shell/harness';

// The browser page is a native view drawn above the window, so it steps aside for a menu of
// Aura's opened over it. A picture of the page stands in its place meanwhile, not an empty area,
// and the page itself comes back when the menu closes. Electron and Core of the E2E build, one
// TEMP workspace page, no network.
async function cleanup(root: string) {
  const target = resolve(root);
  const rel = relative(resolve(tmpdir()), target);
  if (isAbsolute(rel) || rel.startsWith('..') || !rel.startsWith('aura-browser-picture-'))
    throw new Error(`Refusing to remove a non-fixture directory: ${target}`);
  await rm(toNamespacedPath(target), {
    recursive: true,
    force: true,
    maxRetries: 5,
    retryDelay: 200,
  });
}

// eslint-disable-next-line no-empty-pattern -- Electron and Core of the E2E build, TEMP files.
test('a menu over the browser page leaves a picture of the page in its place', async ({}, info) => {
  test.setTimeout(150_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-browser-picture-'));
  const work = join(root, 'workspace');
  await mkdir(work);
  await writeFile(
    join(work, 'page.html'),
    '<!doctype html><meta charset="utf-8"><title>图片页</title>' +
      '<body style="margin:0;background:#2b6cb0"><h1 style="color:#fff">页面</h1></body>',
  );
  const app = await launchAuraWithDataRoot(root).catch(async (error: unknown) => {
    await cleanup(root);
    throw error;
  });
  try {
    const page = await app.firstWindow();
    await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 60_000 });
    const scope = await page.evaluate(async (selected_root) => {
      const workspace = await window.aura.v1.createWorkspace({
        display_name: 'Picture',
        selected_root,
      });
      if (!workspace.ok) throw new Error(workspace.error.message);
      const session = await window.aura.v1.createSession({
        workspace_id: workspace.value.workspace.workspace_id,
        title: 'Picture',
      });
      if (!session.ok) throw new Error(session.error.message);
      return {
        workspaceId: workspace.value.workspace.workspace_id,
        sessionId: session.value.session_id,
      };
    }, work);
    await page.locator('.session-row').filter({ hasText: 'Picture' }).click();
    await page.getByRole('button', { name: '打开工具', exact: true }).click();
    await page
      .getByRole('menu', { name: '选择工具' })
      .getByRole('menuitem', { name: /^浏览器/u })
      .click();
    await page.getByRole('button', { name: '新建页面', exact: true }).click();
    const pages = () => page.evaluate(async () => window.aura.v1.browser!.pages());
    await expect.poll(async () => (await pages()).length).toBeGreaterThan(0);
    const pageId = (await pages()).at(-1)!.pageId;
    const opened = await page.evaluate(
      async ({ scope, pageId }) =>
        window.aura.v1.browser!.command({ kind: 'open-file', ...scope, pageId, path: 'page.html' }),
      { scope, pageId },
    );
    expect(opened.ok, JSON.stringify(opened)).toBe(true);
    const shown = () =>
      app.evaluate(({ BrowserWindow }) =>
        BrowserWindow.getAllWindows()
          .flatMap((window) => window.contentView.children)
          .some((view) => view.getVisible()),
      );
    await expect.poll(shown, { timeout: 30_000 }).toBe(true);
    const picture = page.locator('.browser-viewport img.browser-picture');
    await expect(picture).toHaveCount(0);

    await page
      .getByRole('tablist', { name: '已打开的工具' })
      .getByRole('tab', { name: '浏览器' })
      .click({ button: 'right' });
    await expect(page.getByRole('menu', { name: '浏览器 标签' })).toBeVisible();
    await expect(picture).toBeVisible();
    await expect.poll(shown).toBe(false);
    const drawn = await picture.evaluate((image: HTMLImageElement) => ({
      source: image.src.slice(0, 23),
      width: image.naturalWidth,
      height: image.naturalHeight,
    }));
    await writeFile(info.outputPath('picture.json'), JSON.stringify(drawn, null, 2));
    expect(drawn.source).toBe('data:image/jpeg;base64,');
    expect(drawn.width).toBeGreaterThan(100);
    expect(drawn.height).toBeGreaterThan(100);
    await page.screenshot({ path: info.outputPath('menu-over-picture.png') });

    await page.keyboard.press('Escape');
    await expect(page.getByRole('menu', { name: '浏览器 标签' })).toHaveCount(0);
    await expect.poll(shown).toBe(true);
    await expect(picture).toHaveCount(0);
  } finally {
    await closeAura(app);
    await cleanup(root);
  }
});
