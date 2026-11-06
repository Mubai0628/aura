import { expect, test, type Page } from '@playwright/test';
import { mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pixelAudit, type PixelFinding } from './pixel/audit';
import { launch } from './s06/harness';

const PIXEL = /^"Fusion Pixel 12"/u;
const UI = /^"Inter Variable", "Noto Sans SC Variable"/u;

/** The type the page's own stylesheets give an element built from `html`. */
async function typeOf(page: Page, html: string, selector: string) {
  return page.evaluate(
    ({ html, selector }) => {
      const probe = document.createElement('div');
      probe.innerHTML = html;
      document.body.append(probe);
      try {
        const style = getComputedStyle(probe.querySelector(selector)!);
        return `${style.fontFamily.split(',')[0]} ${style.fontSize} ${style.fontWeight}`;
      } finally {
        probe.remove();
      }
    },
    { html, selector },
  );
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core under the production policy, isolated data root.
test('L18.10 the pixel face ships with Aura and every page keeps the pixel look', async ({}, info) => {
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l18-10-pixel-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const { app, page } = await launch(root);
  try {
    // The face is Aura's own file: `font-src 'self'` refuses one inlined or fetched elsewhere. The
    // startup screen declares it too, for its caption, with the same file.
    const sources = await page.evaluate(() =>
      [...document.styleSheets]
        .flatMap((sheet) => [...sheet.cssRules])
        .filter((rule): rule is CSSFontFaceRule => rule instanceof CSSFontFaceRule)
        .filter(
          (rule) =>
            rule.style.getPropertyValue('font-family').replaceAll('"', '') === 'Fusion Pixel 12',
        )
        .map((rule) => rule.style.getPropertyValue('src')),
    );
    expect(sources.length).toBeGreaterThan(0);
    expect(new Set(sources).size).toBe(1);
    expect(sources[0]).toMatch(/^url\("\.\/fusion-pixel-12px-[^"/]+\.woff2"\) format\("woff2"\)$/u);
    const loaded = await page.evaluate(async () =>
      (await document.fonts.load('12px "Fusion Pixel 12"', '新建会话 设置 Aura 0123')).map(
        (face) => face.status,
      ),
    );
    expect(loaded.length).toBeGreaterThan(0);
    expect(new Set(loaded)).toEqual(new Set(['loaded']));

    // The interface's own words take the pixel face; what the user and the model write, and the
    // fields they write in, keep the reading face.
    expect(
      await typeOf(page, '<nav class="settings-navigation"><button>模型</button></nav>', 'button'),
    ).toMatch(/^"Fusion Pixel 12" 12px 400$/u);
    expect(
      await typeOf(page, '<div class="new-session-welcome"><h1>从一个想法开始</h1></div>', 'h1'),
    ).toMatch(/^"Fusion Pixel 12" 24px 400$/u);
    expect(
      await typeOf(page, '<div class="agent-message-text">回答</div>', '.agent-message-text'),
    ).not.toMatch(PIXEL);
    expect(await page.evaluate(() => getComputedStyle(document.body).fontFamily)).toMatch(UI);
    expect(
      await typeOf(
        page,
        '<div class="settings-form-grid"><label><span>名称</span><input value="aura"></label></div>',
        'input',
      ),
    ).not.toMatch(PIXEL);

    // On screen the pixel face's strokes have grey edges, not ClearType's coloured fringes: the
    // window's own pixels under the sidebar heading, read by Main.
    const heading = await page.locator('.sidebar-heading').first().boundingBox();
    expect(heading).not.toBeNull();
    const edges = await app.evaluate(
      async ({ BrowserWindow }, { url, rect }) => {
        const window = BrowserWindow.getAllWindows().find(
          (each) => !each.isDestroyed() && each.webContents.getURL() === url,
        );
        if (!window) return null;
        const pixels = (await window.webContents.capturePage(rect)).toBitmap();
        let inked = 0;
        let tinted = 0;
        for (let at = 0; at + 3 < pixels.length; at += 4) {
          const [blue, green, red] = [pixels[at]!, pixels[at + 1]!, pixels[at + 2]!];
          if (Math.min(red, green, blue) < 200) inked += 1;
          if (Math.max(red, green, blue) - Math.min(red, green, blue) > 12) tinted += 1;
        }
        return { inked, tinted };
      },
      {
        url: page.url(),
        rect: {
          x: Math.floor(heading!.x),
          y: Math.floor(heading!.y),
          width: Math.ceil(heading!.width),
          height: Math.ceil(heading!.height),
        },
      },
    );
    expect(edges?.inked).toBeGreaterThan(20);
    expect(edges?.tinted).toBe(0);

    // The first screen, then every Settings page, its whole length; what each still breaks once
    // it has settled, all of it reported together.
    const problems: Record<string, PixelFinding[]> = {};
    const check = async (where: string) => {
      let findings = await pixelAudit(page);
      for (let tries = 0; findings.length > 0 && tries < 10; tries += 1) {
        await page.waitForTimeout(500);
        findings = await pixelAudit(page);
      }
      if (findings.length > 0) problems[where] = findings;
    };
    await check('first screen');
    await page.getByRole('button', { name: '设置', exact: true }).click();
    const pages = page.locator('.settings-navigation button');
    await expect(pages.first()).toBeVisible();
    const count = await pages.count();
    expect(count).toBeGreaterThan(5);
    for (let at = 0; at < count; at += 1) {
      const entry = pages.nth(at);
      const name = (await entry.textContent())?.trim() ?? String(at);
      await entry.click();
      await expect(entry).toHaveClass(/is-active/u);
      await check(`设置 → ${name}`);
      await page.screenshot({ path: info.outputPath(`settings-${at}.png`) });
    }
    expect(problems).toEqual({});
  } finally {
    await app.close();
  }
});
