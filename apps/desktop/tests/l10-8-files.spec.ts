import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { finished, launch, pending } from './s06/harness';
import { armReadOnlyWrite } from './s06/setup';
import { writeProvider } from './s06-file-write-provider.mjs';

// The 文件 tab of the right sidebar, read only, through the real Core file
// routes: the tree, text with line numbers, Markdown as a preview and its source, a picture, the
// filter, and a file the model wrote opened from its row in the conversation.

/** A 1×1 PNG. */
const PNG =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';

const lines = (page: import('@playwright/test').Page) =>
  page
    .locator('.files-panel:not([hidden]) .file-line .file-line-text')
    .evaluateAll((items) => items.map((item) => item.textContent));

test('L10.8 文件: tree, line numbers, Markdown preview and source, a picture and the filter', async ({
  browserName,
}, info) => {
  void browserName;
  test.setTimeout(120_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l10-8-files-'));
  const work = join(root, 'workspace');
  await mkdir(join(work, 'src'), { recursive: true });
  await mkdir(join(work, 'assets'));
  await writeFile(join(work, 'README.md'), '# 演示项目\n\n一个用于测试的项目。\n');
  await writeFile(join(work, 'src', 'main.rs'), 'fn main() {\n    println!("hello");\n}\n');
  await writeFile(join(work, 'assets', 'logo.png'), Buffer.from(PNG, 'base64'));
  const { app, page } = await launch(root);
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  try {
    await page.evaluate(async (selected_root) => {
      const workspace = await window.aura.v1.createWorkspace({
        display_name: 'L10.8 files',
        selected_root,
      });
      if (!workspace.ok) throw new Error(workspace.error.message);
      const session = await window.aura.v1.createSession({
        workspace_id: workspace.value.workspace.workspace_id,
        title: 'L10.8 files',
      });
      if (!session.ok) throw new Error(session.error.message);
    }, work);
    await page.locator('.session-row').filter({ hasText: 'L10.8 files' }).click();
    await page.getByRole('button', { name: '打开工具', exact: true }).click();
    await page
      .getByRole('menu', { name: '选择工具' })
      .getByRole('menuitem', { name: /^文件/u })
      .click();
    // With no file open the tree has the whole page.
    await expect(page.locator('.files-panel:not([hidden]) .files-layout')).toHaveAttribute(
      'data-view',
      'none',
    );
    const tree = page.getByRole('navigation', { name: '文件树' });
    const src = tree.getByRole('button', { name: 'src', exact: true });
    await expect(src).toBeVisible({ timeout: 15_000 });
    await expect(tree.getByRole('button', { name: 'README.md', exact: true })).toBeVisible();

    // A file picked in the empty tab opens there, with its lines numbered.
    await src.click();
    await tree.getByRole('button', { name: 'main.rs', exact: true }).click();
    await expect(page.getByRole('tab', { name: 'main.rs' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    await expect.poll(() => lines(page)).toEqual(['fn main() {', '    println!("hello");', '}']);
    // The tree marks the file shown.
    await expect(tree.getByRole('button', { name: 'main.rs', exact: true })).toHaveAttribute(
      'data-open',
      'true',
    );
    await expect(page.locator('.files-panel:not([hidden]) .file-line-number').first()).toHaveText(
      '1',
    );

    // Another file opens beside it, a second small tab of the same 文件 page: Markdown as
    // a preview, its source one button away.
    await tree.getByRole('button', { name: 'README.md', exact: true }).click();
    await expect(page.getByRole('tab', { name: 'README.md' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    await expect(page.getByRole('heading', { level: 1, name: '演示项目' })).toBeVisible();
    await page.getByRole('button', { name: '查看源码' }).click();
    await expect.poll(async () => (await lines(page))[0]).toBe('# 演示项目');
    await page.screenshot({ path: info.outputPath('files-markdown-source.png') });

    // A picture shows as itself (the production CSP allows data: images).
    await tree.getByRole('button', { name: 'assets', exact: true }).click();
    await tree.getByRole('button', { name: 'logo.png', exact: true }).click();
    const image = page.getByRole('img', { name: 'logo.png' });
    await expect(image).toBeVisible();
    await expect.poll(() => image.evaluate((item: HTMLImageElement) => item.naturalWidth)).toBe(1);

    // The filter finds a file by name; picking it shows its tab again.
    await tree.getByRole('searchbox', { name: '筛选文件' }).fill('main');
    await tree.getByRole('button', { name: 'src/main.rs', exact: true }).click();
    await expect(page.getByRole('tab', { name: 'main.rs' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    // One sidebar tab, 文件; the three files are that page's own small tabs.
    await expect(
      page.getByRole('tablist', { name: '已打开的工具' }).getByRole('tab', { name: '文件' }),
    ).toHaveCount(1);
    await expect(page.getByRole('tablist', { name: '打开的文件' }).getByRole('tab')).toHaveCount(3);

    // The tree hides and shows with one button.
    await page
      .locator('.files-panel:not([hidden])')
      .getByRole('button', { name: '隐藏文件树' })
      .click();
    await expect(tree).toHaveCount(0);
    await page.screenshot({ path: info.outputPath('files-tree-hidden.png') });
    expect(errors).toEqual([]);
  } finally {
    await app.close();
  }
});

test('L10.8 文件: the file the model wrote opens from its row in the conversation', async ({
  browserName,
}) => {
  void browserName;
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l10-8-files-link-'));
  const work = join(root, 'workspace');
  await mkdir(work, { recursive: true });
  const provider = await writeProvider({ path: 'hello.md', text: '# 你好\n\nAura 写入。\n' });
  const { app, page } = await launch(root);
  try {
    const setup = await armReadOnlyWrite(page, work, provider.url, {
      title: 'L10.8 file link',
      text: '写一个说明文件',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await pending(page, session);
    await page
      .locator('section[aria-label="当前任务审批"]')
      .getByRole('button', { name: '仅此一次', exact: true })
      .click();
    await finished(page, session);
    expect(await readFile(join(work, 'hello.md'), 'utf8')).toContain('你好');
    await page.locator('.session-row').first().click();
    // A finished turn folds its process behind its status line; open the turn, then its
    // process section if that is closed.
    const turn = page.locator('.agent-turn-toggle').first();
    await expect(turn).toBeVisible();
    if ((await turn.getAttribute('aria-expanded')) === 'false') await turn.click();
    const process = page.locator('.agent-process-toggle').first();
    await expect(process).toBeVisible();
    if ((await process.getAttribute('aria-expanded')) === 'false') await process.click();
    await page.getByRole('button', { name: '打开 hello.md', exact: true }).click();
    await expect(page.getByRole('tab', { name: 'hello.md' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    await expect(page.getByRole('heading', { level: 1, name: '你好' })).toBeVisible();
    expect(provider.errors).toEqual([]);
  } finally {
    await app.close();
    await provider.close();
  }
});
