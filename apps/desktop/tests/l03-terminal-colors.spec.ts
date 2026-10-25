import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { launch } from './s06/harness';
import { chooseDefaultTerminal, openTerminalTab } from './l10-8/terminal-tabs';

// Earlier builds had RGB colours blocked as inline style attributes, and pale yellow on the
// light canvas.
test('real ConPTY output uses readable ANSI hues and indexed RGB without style-attribute CSP errors', async () => {
  const info = test.info();
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l03-colors-'));
  const work = join(root, 'workspace');
  await mkdir(work);
  const codes = [31, 32, 33, 34, 35, 36, 91, 92, 93, 94, 95, 96];
  const hues: Record<number, string> = {
    31: 'rgb(180, 35, 24)',
    32: 'rgb(47, 107, 57)',
    33: 'rgb(135, 97, 0)',
    34: 'rgb(37, 88, 163)',
    35: 'rgb(133, 65, 141)',
    36: 'rgb(28, 108, 117)',
    91: 'rgb(207, 53, 44)',
    92: 'rgb(59, 123, 66)',
    93: 'rgb(150, 112, 0)',
    94: 'rgb(52, 106, 190)',
    95: 'rgb(154, 75, 163)',
    96: 'rgb(38, 123, 134)',
  };
  const script =
    [
      '@echo off',
      ...codes.map((code) => `echo \x1b[${code}mL03-ANSI-${code}\x1b[0m`),
      'echo \x1b[38;2;175;0;95mL03-RGB-FG\x1b[0m',
      'echo \x1b[48;2;0;135;175mL03-RGB-BG\x1b[0m',
      'echo L03-COLORS-DONE',
    ].join('\r\n') + '\r\n';
  await writeFile(join(work, 'l03-colors.cmd'), script, 'utf8');
  const { app, page } = await launch(root, { AURA_TERMINAL_TEST_NO_PROFILE: '1' });
  try {
    await page.evaluate(async (selected_root) => {
      const workspace = await window.aura.v1.createWorkspace({
        display_name: 'L03 colors',
        selected_root,
      });
      if (!workspace.ok) throw new Error(workspace.error.message);
      const session = await window.aura.v1.createSession({
        workspace_id: workspace.value.workspace.workspace_id,
        title: 'L03 colors',
      });
      if (!session.ok) throw new Error(session.error.message);
    }, work);
    await page.locator('.session-row').filter({ hasText: 'L03 colors' }).click();
    await chooseDefaultTerminal(page, 'CMD');
    await openTerminalTab(page);
    await expect(page.locator('.xterm')).toHaveCount(1, { timeout: 60_000 });
    // Give the panel enough rows that all twelve ANSI and both RGB samples stay visible.
    await app.evaluate(({ BrowserWindow }) =>
      BrowserWindow.getAllWindows()[0]?.setSize(1400, 1100),
    );
    await page.locator('.terminal-host').click();
    await page.keyboard.type('echo L03-COLOR-READY');
    await page.keyboard.press('Enter');
    const screen = page.locator('.xterm-accessibility-tree');
    await expect
      .poll(async () =>
        (await screen.innerText()).split('\n').some((row) => row.trim() === 'L03-COLOR-READY'),
      )
      .toBe(true);
    await page.evaluate(() => {
      const state = window as typeof window & { l03ColorCsp: string[] };
      state.l03ColorCsp = [];
      window.addEventListener('securitypolicyviolation', (event) => {
        // The inherited stylesheet mirror still produces style-src-elem events. RGB used to
        // create style-src-attr violations per rendered cell; those are the boundary under test.
        if (event.effectiveDirective === 'style-src-attr')
          state.l03ColorCsp.push(event.violatedDirective);
      });
    });
    // Execute the temp fixture through CMD -> real Console/ConPTY -> Core -> projection -> xterm.
    await page.keyboard.type('call l03-colors.cmd');
    await page.keyboard.press('Enter');
    await expect(screen).toContainText('L03-COLORS-DONE', { timeout: 20_000 });
    const samples = await page.locator('.xterm-rows span').evaluateAll((items) => {
      const rgb = (value: string) =>
        value
          .match(/[\d.]+/g)
          ?.slice(0, 3)
          .map(Number) ?? [];
      const luminance = (value: string) => {
        const [r, g, b] = rgb(value).map((part) => {
          const channel = part / 255;
          return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
        });
        return r! * 0.2126 + g! * 0.7152 + b! * 0.0722;
      };
      const canvas = getComputedStyle(document.querySelector('.xterm-viewport')!).backgroundColor;
      const background = luminance(canvas);
      return items
        .filter((item) => /L03-(?:ANSI|RGB)-/.test(item.textContent ?? ''))
        .map((item) => {
          const element = item as HTMLElement;
          const style = getComputedStyle(element);
          const foreground = luminance(style.color);
          return {
            text: element.textContent?.trim(),
            classes: element.className,
            foreground: style.color,
            background: style.backgroundColor,
            inlineColor: element.style.color,
            inlineBackground: element.style.backgroundColor,
            contrast:
              (Math.max(foreground, background) + 0.05) / (Math.min(foreground, background) + 0.05),
          };
        });
    });
    for (const code of codes) {
      const sample = samples.find((item) => item.text === `L03-ANSI-${code}`);
      expect(sample, `visible ANSI ${code}`).toBeDefined();
      // Contrast alone could pass if a blocked stylesheet made every sample use default ink.
      expect(sample!.foreground, `ANSI ${code} palette applied`).toBe(hues[code]);
      expect(sample!.classes.split(' ')).toContain(
        `xterm-fg-${code >= 90 ? code - 82 : code - 30}`,
      );
      expect(sample!.contrast, `ANSI ${code} contrast`).toBeGreaterThanOrEqual(3);
    }
    const fg = samples.find((item) => item.text === 'L03-RGB-FG');
    const bg = samples.find((item) => item.text === 'L03-RGB-BG');
    expect(fg?.classes).toContain('xterm-fg-125');
    expect(bg?.classes).toContain('xterm-bg-31');
    expect(fg?.foreground).toBe('rgb(175, 0, 95)');
    expect(bg?.background).toBe('rgb(0, 135, 175)');
    expect(samples.every((item) => item.inlineColor === '' && item.inlineBackground === '')).toBe(
      true,
    );
    expect(
      await page.evaluate(() => (window as typeof window & { l03ColorCsp: string[] }).l03ColorCsp),
    ).toEqual([]);
    await info.attach('real-conpty-colors', {
      body: JSON.stringify(samples, null, 2),
      contentType: 'application/json',
    });
    const screenshot = info.outputPath('terminal-colors.png');
    await page.screenshot({ path: screenshot });
    await info.attach('terminal-colors', { path: screenshot, contentType: 'image/png' });
  } finally {
    await app.close();
  }
});
