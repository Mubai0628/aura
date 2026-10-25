import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { answer, finished, history, launch } from './s06/harness';
import { writeProvider } from './s06-file-write-provider.mjs';
import { expectSecureSurface } from './electron-shell/renderer-api';
import { copiedThroughMain, stubMainClipboard } from './l10-12/main-clipboard';

// In the real window with the real Core; only the model is a local script.
// While the model writes a whole file into one tool call, the turn says which tool, which
// file and how much has arrived, growing. Also: 复制 under the answer, the composer's controls one
// size, and the context bar drawn finer than the screen's pixels, cheaply.

const PAGE = [
  '<!doctype html>',
  '<html lang="zh-CN"><head><meta charset="utf-8"><title>鹈鹕骑自行车</title></head><body>',
  ...Array.from(
    { length: 480 },
    (_, line) => `<p>第 ${line + 1} 段：一只鹈鹕骑着自行车穿过海边。</p>`,
  ),
  '</body></html>',
].join('\n');
const ANSWER = '已写好 **l10-10-pelican.html**：\n\n- 480 段文字\n- 打开即可查看';
const WRITING = /^正在编写「写入文件」l10-10-pelican\.html · 已生成 ([\d.]+) KB$/u;

// eslint-disable-next-line no-empty-pattern -- real Electron/Core in TEMP; scripted loopback model only.
test('L10.10 a file being written shows its size growing; 复制, even controls, a fine and cheap context bar', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l10-10-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  // The call's arguments arrive in 24 pieces over about four seconds, as a slow model's would.
  const model = await writeProvider({
    path: 'l10-10-pelican.html',
    text: PAGE,
    streamArguments: { chunks: 24, every: 160 },
    finalText: ANSWER,
  });
  const { app, page } = await launch(root);
  try {
    await expectSecureSurface(page);
    const release = model.holdNext();
    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L10.10',
      text: '写一个鹈鹕骑自行车的网页',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await page.locator('.session-row').first().click();
    await expect(page.locator('.agent-turn-spinner')).toBeVisible();
    release();

    // While the arguments stream: the tool, the file and the size, growing; the old fixed hint
    // never shows; the words carry the light, not the row.
    const seen: string[] = [];
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      const now = await page.evaluate(() => ({
        activity: document.querySelector('.agent-step-activity')?.textContent ?? null,
        clip: (() => {
          const element = document.querySelector('.agent-step-activity');
          if (!element) return null;
          const style = getComputedStyle(element);
          return style.backgroundClip || style.getPropertyValue('-webkit-background-clip');
        })(),
        stale: document.body.textContent?.includes('正文仍在生成') ?? false,
      }));
      expect(now.stale, 'the old fixed hint').toBe(false);
      if (now.activity && WRITING.test(now.activity)) {
        expect(now.clip).toBe('text');
        if (seen.at(-1) !== now.activity) seen.push(now.activity);
      }
      if ((await history(page, session)).active?.pending_approval_ids.length) break;
      await page.waitForTimeout(80);
    }
    await info.attach('activity-seen', { body: seen.join('\n'), contentType: 'text/plain' });
    console.log(`L10.10 writing: ${seen.join(' | ')}`);
    const sizes = seen.map((text) => Number(WRITING.exec(text)![1]));
    expect(sizes.length, seen.join(' | ')).toBeGreaterThanOrEqual(3);
    for (let index = 1; index < sizes.length; index += 1)
      expect(sizes[index]!).toBeGreaterThan(sizes[index - 1]!);

    // The write goes through once allowed, and the turn ends with the answer.
    await answer(page, session, 'allow_once');
    await finished(page, session);
    expect(await readFile(join(workspace, 'l10-10-pelican.html'), 'utf8')).toBe(PAGE);
    await expect(page.locator('.agent-step-activity')).toHaveCount(0);
    await page.screenshot({ path: info.outputPath('l10-10-finished.png') });

    // 复制 copies the whole answer as written, through Main. Main's clipboard is replaced first, and nothing is clicked unless it was:
    // the owner's clipboard is never touched.
    await stubMainClipboard(app);
    const report = page.locator('.agent-turn').last();
    await report.getByRole('button', { name: '复制', exact: true }).last().click();
    await expect(report.getByRole('button', { name: '已复制', exact: true })).toBeVisible();
    expect(await copiedThroughMain(app)).toEqual([ANSWER]);

    // The composer's controls are one size: 28px squares, their icons 16px.
    const controls = await page.evaluate(() => {
      const box = (element: Element | null) => {
        const rect = element?.getBoundingClientRect();
        return rect ? [Math.round(rect.width), Math.round(rect.height)] : null;
      };
      const add = document.querySelector('button[aria-label="添加"]');
      return {
        add: box(add),
        addIcon: box(add?.querySelector('svg') ?? null),
        context: box(document.querySelector('.composer-context')),
        send: box(document.querySelector('.composer .send-button')),
        chips: [...document.querySelectorAll('.composer .composer-select')].map(
          (chip) => box(chip)?.[1],
        ),
      };
    });
    expect(controls).toMatchObject({
      add: [28, 28],
      addIcon: [16, 16],
      context: [28, 28],
      send: [28, 28],
    });
    expect(controls.chips.length).toBeGreaterThan(0);
    for (const height of controls.chips) expect(height).toBe(28);

    // The context card: rows of pixel cells, nothing spilling over its edge.
    await page.getByRole('button', { name: /^上下文占用/u }).click();
    const bar = page.getByRole('region', { name: '上下文占用详情' });
    await expect(bar.locator('.context-pixel-head')).toContainText('上下文');
    expect(await bar.evaluate((section) => section.scrollWidth <= section.clientWidth)).toBe(true);
    await page.screenshot({ path: info.outputPath('l10-10-context-bar.png') });
    expect(model.errors).toEqual([]);
  } finally {
    await app.close();
    await model.close();
  }
});
