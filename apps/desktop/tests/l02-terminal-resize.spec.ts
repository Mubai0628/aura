import { expect, test, type Page } from '@playwright/test';
import { mkdtemp, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { launch } from './s06/harness';
import { chooseDefaultTerminal, openTerminalTab } from './l10-8/terminal-tabs';

// Two resize faults. First: squeezing the terminal by dragging the panel divider and
// widening it again left many garbled, duplicated copies of the screen in scrollback. ConPTY
// repaints its screen after every resize; xterm resized on every frame while ConPTY lagged behind.
// Second: dragging the panel to its narrowest (past the snap point) and back without
// releasing cut off everything above the last few lines. ConPTY keeps one screen and repaints it
// from the top after a resize, over rows where xterm still showed history. Hand-like drags (about
// one frame per step) reproduce both; the whole history is read back page by page.

const numbered = (first: number) =>
  Array.from({ length: 70 }, (_, index) => `N${first + index} ${'0123456789'.repeat(7)} END`);

// Every row of the buffer, read through the rendered rows from the top down.
async function history(page: Page) {
  await page.locator('.terminal-host').click();
  const rows = page.locator('.xterm-rows');
  const read = async () =>
    (await rows.innerText())
      .replaceAll(' ', ' ')
      .split('\n')
      .map((row) => row.trimEnd());
  let previous = '';
  for (let press = 0; press < 30; press++) {
    await page.keyboard.press('Shift+PageUp');
    await page.waitForTimeout(150);
    const current = (await read()).join('\n');
    if (current === previous) break;
    previous = current;
  }
  const all: string[] = [];
  for (let press = 0; press < 30; press++) {
    const view = await read();
    let overlap = Math.min(all.length, view.length);
    while (overlap > 0 && all.slice(-overlap).join('\n') !== view.slice(0, overlap).join('\n')) {
      overlap--;
    }
    all.push(...view.slice(overlap));
    await page.keyboard.press('Shift+PageDown');
    await page.waitForTimeout(150);
    if ((await read()).join('\n') === view.join('\n')) break;
  }
  return all;
}

for (const shell of ['powershell', 'cmd'] as const) {
  test(`${shell}: resizing the terminal keeps every line once and whole`, async () => {
    const lines = numbered(shell === 'powershell' ? 101 : 201);
    test.setTimeout(240_000);
    const root = await mkdtemp(join(tmpdir(), 'aura-l02-resize-'));
    const work = join(root, 'workspace');
    await mkdir(work);
    const { app, page } = await launch(root, { AURA_TERMINAL_TEST_NO_PROFILE: '1' }, undefined, [
      '--disable-features=CalculateNativeWinOcclusion',
      '--disable-background-timer-throttling',
      '--disable-renderer-backgrounding',
    ]);
    try {
      await page.evaluate(async (selected_root) => {
        const workspace = await window.aura.v1.createWorkspace({
          display_name: 'L02 resize',
          selected_root,
        });
        if (!workspace.ok) throw new Error(workspace.error.message);
        const created = await window.aura.v1.createSession({
          workspace_id: workspace.value.workspace.workspace_id,
          title: 'L02 resize',
        });
        if (!created.ok) throw new Error(created.error.message);
      }, work);
      // A wide window, like the owner's, so the panel can be squeezed and widened a lot.
      await app.evaluate(({ BrowserWindow }) => {
        const window = BrowserWindow.getAllWindows()[0];
        // These are foreground-like 16ms drags. Another desktop window must not
        // turn Chromium's input acknowledgements into one background frame/second.
        window?.webContents.setBackgroundThrottling(false);
        window?.setSize(1800, 800);
      });
      // A drag ends when the window loses focus (CollapsiblePaneResizer); count the times another
      // desktop window took it, so a failed resize says whether that happened.
      await page.evaluate(() => {
        const counted = window as unknown as { auraBlurs: number };
        counted.auraBlurs = 0;
        window.addEventListener('blur', () => {
          counted.auraBlurs += 1;
        });
      });
      await page.locator('.session-row').filter({ hasText: 'L02 resize' }).click();
      if (shell === 'cmd') await chooseDefaultTerminal(page, 'CMD');
      await openTerminalTab(page);
      await expect(page.locator('.xterm')).toHaveCount(1, { timeout: 60_000 });
      const divider = page.getByRole('separator', { name: '调整右侧工作区宽度' });
      let mouseX = 0;
      let mouseY = 0;
      const press = async () => {
        const box = await divider.boundingBox();
        if (!box) throw new Error('missing sidebar divider');
        mouseX = box.x + box.width / 2;
        mouseY = box.y + box.height / 2;
        await page.mouse.move(mouseX, mouseY);
        await page.mouse.down();
      };
      const move = async (dx: number, steps = 60) => {
        const from = mouseX;
        for (let step = 1; step <= steps; step++) {
          await page.mouse.move(from + (dx * step) / steps, mouseY);
          await page.waitForTimeout(16);
        }
        mouseX = from + dx;
      };
      const drag = async (dx: number) => {
        await press();
        await move(dx);
        await page.mouse.up();
      };
      const terminalWidth = async () =>
        Math.round((await page.locator('.xterm').boundingBox())?.width ?? 0);
      await drag(-800);
      await page.waitForTimeout(500);
      await page.locator('.terminal-host').click();
      await page.keyboard.type(
        shell === 'powershell'
          ? "101..170 | ForEach-Object { 'N' + $_ + ' ' + ('0123456789' * 7) + ' END' }"
          : `for /L %i in (201,1,270) do @echo N%i ${'0123456789'.repeat(7)} END`,
      );
      await page.keyboard.press('Enter');
      const screen = page.locator('.xterm-accessibility-tree');
      await expect(screen).toContainText(lines.at(-1) ?? 'END', { timeout: 20_000 });
      const wide = await terminalWidth();
      const settled: number[] = [];
      const settle = async (check: (width: number) => boolean) => {
        const started = Date.now();
        try {
          await expect
            .poll(async () => check(await terminalWidth()), { timeout: 10_000 })
            .toBe(true);
        } catch (error) {
          await test.info().attach('failed-resize-dimensions', {
            body: JSON.stringify({
              wide,
              terminal: await terminalWidth(),
              viewport: await page.evaluate(() => ({
                width: innerWidth,
                height: innerHeight,
                hidden: document.hidden,
                focused: document.hasFocus(),
                blurs: (window as unknown as { auraBlurs: number }).auraBlurs,
              })),
            }),
            contentType: 'application/json',
          });
          throw error;
        }
        settled.push(Date.now() - started);
      };
      // To the panel minimum, held there, past the snap point, held, and back
      // out without releasing. The terminal waits for the release; nothing changes size.
      await press();
      await move(wide - 230);
      await page.waitForTimeout(400);
      await move(120, 20);
      await page.waitForTimeout(400);
      await move(-(wide - 230) - 120);
      await page.mouse.up();
      await page.waitForTimeout(500);
      await settle((width) => Math.abs(width - wide) < 40);
      // Released at the narrowest and widened later, three times (the first report's drag).
      for (let round = 0; round < 3; round++) {
        // Close to the panel's 264 px minimum; going further collapses the panel.
        await drag(Math.max(0, (await terminalWidth()) - 250));
        await settle((width) => width < wide / 2);
        await page.waitForTimeout(300);
        await drag(-800);
        await settle((width) => width > wide / 2);
        await page.waitForTimeout(300);
      }
      // The window itself, narrowed and widened.
      const resizeWindow = async (width: number) => {
        const dimensions = await app.evaluate(async ({ BrowserWindow }, requested) => {
          const window = BrowserWindow.getAllWindows()[0];
          if (!window) throw new Error('temporary resize window is unavailable');
          const before = {
            bounds: window.getBounds(),
            maximized: window.isMaximized(),
            fullscreen: window.isFullScreen(),
          };
          // setSize only updates normal bounds for a maximized window. Measure
          // normal-window resizes even when Windows has maximized this fixture.
          if (window.isMaximized()) {
            await new Promise<void>((resolve) => {
              window.once('unmaximize', resolve);
              window.unmaximize();
            });
          }
          window.setSize(requested, 800);
          return { before, requested, after: window.getBounds() };
        }, width);
        await test.info().attach(`native-window-${width}`, {
          body: JSON.stringify(dimensions),
          contentType: 'application/json',
        });
      };
      await resizeWindow(900);
      await settle((width) => width < wide * 0.8);
      await page.waitForTimeout(500);
      await resizeWindow(1800);
      await settle((width) => width > wide * 0.8);
      await page.waitForTimeout(1_000);
      await test.info().attach('resize-settle-ms', {
        body: JSON.stringify(settled),
        contentType: 'application/json',
      });
      const blurs = await page.evaluate(
        () => (window as unknown as { auraBlurs: number }).auraBlurs,
      );
      console.log(`L02-RESIZE-SETTLE ${shell} ${JSON.stringify(settled)} blurs=${blurs}`);
      // Every line once, whole and in order: before the fix the scrollback was full of repeated,
      // half-overwritten screens, and later a narrowed-and-widened panel lost about 25 lines.
      const rows = await history(page);
      await test.info().attach('history', { body: rows.join('\n'), contentType: 'text/plain' });
      // A cut-off piece of a line shows up as a row of digits ending in END.
      const listed = rows.filter((row) => /^N\d{3} /u.test(row) || /^\d+ END$/u.test(row));
      expect(listed).toEqual(lines);
      // The shell still answers.
      await page.keyboard.type(
        shell === 'powershell' ? "'L02-AFTER' + '-RESIZE'" : 'echo L02-AFTER-RESIZE',
      );
      await page.keyboard.press('Enter');
      await expect
        .poll(async () =>
          (await screen.innerText()).split('\n').some((row) => row.trim() === 'L02-AFTER-RESIZE'),
        )
        .toBe(true);
    } finally {
      await app.close();
    }
  });
}
