import { expect, test, type Page } from '@playwright/test';
import { mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { launch } from './s06/harness';

/** The font set: Inter with 思源黑体 for the interface and the conversation,
 * 思源宋体 for large titles and thinking, JetBrains Mono for code. */
const UI = /^"Inter Variable", "Noto Sans SC Variable"/;
const SERIF = /^"Noto Serif SC Variable"/;
const MONO = /^"JetBrains Mono Variable"/;

/** The font family the page's own stylesheets give an element built from `html`. */
async function familyOf(page: Page, html: string, selector: string) {
  return page.evaluate(
    ({ html, selector }) => {
      const probe = document.createElement('div');
      probe.innerHTML = html;
      document.body.append(probe);
      try {
        return getComputedStyle(probe.querySelector(selector)!).fontFamily;
      } finally {
        probe.remove();
      }
    },
    { html, selector },
  );
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core under the production policy, isolated data root.
test('L10.7 the chosen fonts ship with Aura and load under its security policy', async ({}, info) => {
  test.setTimeout(120_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l10-7-fonts-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const { app, page } = await launch(root);
  try {
    // Every face comes from Aura itself: an inlined `data:` font is refused by `font-src 'self'`.
    const sources = await page.evaluate(() =>
      [...document.styleSheets]
        .flatMap((sheet) => [...sheet.cssRules])
        .filter((rule): rule is CSSFontFaceRule => rule instanceof CSSFontFaceRule)
        .map((rule) => ({
          family: rule.style.getPropertyValue('font-family').replaceAll('"', ''),
          src: rule.style.getPropertyValue('src'),
        })),
    );
    const families = new Set(sources.map((face) => face.family));
    for (const family of [
      'Inter Variable',
      'Noto Sans SC Variable',
      'Noto Serif SC Variable',
      'JetBrains Mono Variable',
    ]) {
      expect(families, family).toContain(family);
    }
    expect(sources.filter((face) => face.src.includes('data:'))).toEqual([]);

    // Each family loads the characters it is used for.
    const loaded = await page.evaluate(async () => {
      const samples: [string, string][] = [
        ['Inter Variable', 'Aura 0O 1lI'],
        ['Noto Sans SC Variable', '新建会话 查看详情'],
        ['Noto Serif SC Variable', '从一个想法开始'],
        ['JetBrains Mono Variable', 'fn main() {}'],
      ];
      const result: Record<string, string> = {};
      for (const [family, text] of samples) {
        const faces = await document.fonts.load(`16px "${family}"`, text);
        result[family] =
          faces.length === 0
            ? 'none'
            : faces.every((f) => f.status === 'loaded')
              ? 'loaded'
              : 'error';
      }
      return result;
    });
    expect(loaded).toEqual({
      'Inter Variable': 'loaded',
      'Noto Sans SC Variable': 'loaded',
      'Noto Serif SC Variable': 'loaded',
      'JetBrains Mono Variable': 'loaded',
    });

    // Who uses which: the interface and the conversation, large titles, thinking, code.
    expect(await page.evaluate(() => getComputedStyle(document.body).fontFamily)).toMatch(UI);
    expect(
      await familyOf(page, '<div class="agent-message-text">回答</div>', '.agent-message-text'),
    ).toMatch(UI);
    expect(
      await familyOf(page, '<div class="new-session-welcome"><h1>从一个想法开始</h1></div>', 'h1'),
    ).toMatch(SERIF);
    expect(
      await familyOf(page, '<div class="agent-reasoning">先读 README</div>', '.agent-reasoning'),
    ).toMatch(SERIF);
    expect(
      await familyOf(
        page,
        '<div class="markdown-body"><pre><code>fn main()</code></pre></div>',
        'code',
      ),
    ).toMatch(MONO);
    await page.getByRole('button', { name: '设置', exact: true }).click();
    const heading = page.locator('.settings-content-heading h2').first();
    await expect(heading).toBeVisible();
    expect(await heading.evaluate((element) => getComputedStyle(element).fontFamily)).toMatch(
      SERIF,
    );
  } finally {
    await app.close();
  }
});
