import { expect, test, type Page } from '@playwright/test';
import { mkdtemp, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { launch } from './s06/harness';
import { openTerminalTab } from './l10-8/terminal-tabs';

// Ctrl+V in the terminal once showed "无法读取剪贴板" and pasted
// nothing, so the colour command was typed by hand (with a stray space) and printed no colours.
// The terminal read the clipboard through navigator.clipboard, which Main's permission handler
// refuses; the browser's own paste needs no permission. This test uses the real Windows
// clipboard, so whatever it held is kept in Main's memory and put back afterwards.
//
// The clipboard belongs to whoever is at the desk, so ordinary and full E2E runs skip this test
// unless AURA_E2E_SYSTEM_CLIPBOARD=1 asks for it.
test.skip(
  process.env.AURA_E2E_SYSTEM_CLIPBOARD !== '1',
  'uses the real Windows clipboard; set AURA_E2E_SYSTEM_CLIPBOARD=1 to run it',
);

const colours = ['Red', 'Green', 'Yellow', 'Blue', 'Magenta', 'Cyan'] as const;

// The rendered row whose whole text is `text`.
const row = (page: Page, text: string) =>
  page.locator('.xterm-rows > div').filter({ hasText: new RegExp(`^\\s*${text}\\s*$`, 'u') });

test('terminal: Ctrl+V pastes, Ctrl+C copies the selection, and colours render', async () => {
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l03-clipboard-'));
  const work = join(root, 'workspace');
  await mkdir(work);
  const { app, page } = await launch(root, { AURA_TERMINAL_TEST_NO_PROFILE: '1' });
  // Electron 44 writes only newly constructed ClipboardItems, so keep each item's data.
  await app.evaluate(async ({ clipboard }) => {
    const items = await clipboard.read();
    const saved = await Promise.all(
      items.map(async (item) =>
        Object.fromEntries(
          await Promise.all(item.types.map(async (type) => [type, await item.getType(type)])),
        ),
      ),
    );
    Object.assign(globalThis, { auraTestClipboard: saved });
  });
  const clipboardText = () => app.evaluate(({ clipboard }) => clipboard.readText());
  const setClipboard = (text: string) =>
    app.evaluate(({ clipboard }, value) => clipboard.writeText(value), text);
  try {
    await page.evaluate(async (selected_root) => {
      const workspace = await window.aura.v1.createWorkspace({
        display_name: 'L03 clipboard',
        selected_root,
      });
      if (!workspace.ok) throw new Error(workspace.error.message);
      const created = await window.aura.v1.createSession({
        workspace_id: workspace.value.workspace.workspace_id,
        title: 'L03 clipboard',
      });
      if (!created.ok) throw new Error(created.error.message);
    }, work);
    await page.locator('.session-row').filter({ hasText: 'L03 clipboard' }).click();
    await openTerminalTab(page);
    await expect(page.locator('.xterm')).toHaveCount(1, { timeout: 60_000 });
    const screen = page.locator('.xterm-accessibility-tree');
    await expect(screen).toContainText('PS ', { timeout: 30_000 });
    const failure = page.getByText(/无法(读取|写入)剪贴板/u);

    // Paste: Ctrl+V puts the clipboard on the prompt line, and the shell runs it.
    const pasted = "Write-Output ('PASTE' + '-OK-' + 4242)";
    await setClipboard(pasted);
    await page.locator('.terminal-host').click();
    await page.keyboard.press('Control+V');
    await expect(screen).toContainText(pasted, { timeout: 10_000 });
    await expect(failure).toHaveCount(0);
    await page.keyboard.press('Enter');
    await expect(row(page, 'PASTE-OK-4242')).toHaveCount(1, { timeout: 10_000 });

    // Copy: a double-click selects the word, and Ctrl+C copies it instead of interrupting.
    await setClipboard('before-copy');
    const box = await row(page, 'PASTE-OK-4242').boundingBox();
    if (!box) throw new Error('missing PASTE-OK-4242 row');
    await page.mouse.dblclick(box.x + 30, box.y + box.height / 2);
    await page.keyboard.press('Control+C');
    await expect.poll(clipboardText, { timeout: 5_000 }).toBe('PASTE-OK-4242');
    await expect(failure).toHaveCount(0);

    // The owner's colour command, pasted as they would: every name in its own colour.
    await setClipboard(
      `${colours.map((name) => `'${name}'`).join(',')} | % { Write-Host " $_ " -ForegroundColor $_ }`,
    );
    await page.locator('.terminal-host').click();
    await page.keyboard.press('Control+V');
    await page.keyboard.press('Enter');
    for (const name of colours) await expect(row(page, name)).toHaveCount(1, { timeout: 10_000 });
    const painted = await page.evaluate((names) => {
      const rows = [...document.querySelectorAll<HTMLElement>('.xterm-rows > div')];
      const colourOf = (name: string) => {
        const line = rows.find((element) => element.textContent?.trim() === name);
        const span = [...(line?.querySelectorAll('span') ?? [])].find(
          (element) => element.textContent?.trim() === name,
        );
        return span ? getComputedStyle(span).color : 'missing';
      };
      const prompt = rows.find((element) => element.textContent?.startsWith('PS '));
      return {
        plain: prompt ? getComputedStyle(prompt).color : 'missing',
        named: names.map(colourOf),
      };
    }, colours);
    await test.info().attach('colours', {
      body: JSON.stringify(painted),
      contentType: 'application/json',
    });
    console.log(`L03-CLIPBOARD-COLOURS ${JSON.stringify(painted)}`);
    expect(painted.named).not.toContain('missing');
    expect(painted.named).not.toContain(painted.plain);
    expect(new Set(painted.named).size).toBe(colours.length);
  } finally {
    await app
      .evaluate(async ({ clipboard, ClipboardItem }) => {
        const { auraTestClipboard: saved } = globalThis as {
          auraTestClipboard?: ConstructorParameters<typeof ClipboardItem>[0][];
        };
        if (!saved) return;
        await (saved.length > 0
          ? clipboard.write(saved.map((entry) => new ClipboardItem(entry)))
          : clipboard.clear());
      })
      .catch((error: unknown) => console.error('restoring the clipboard failed', error));
    await app.close();
  }
});
