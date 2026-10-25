import { expect, test } from '@playwright/test';
import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { armReadOnlyWrite } from './s06/setup';
import { answer, finished, launch } from './s06/harness';
import { writeProvider } from './s06-file-write-provider.mjs';
import { expectSecureSurface } from './electron-shell/renderer-api';
import { copiedThroughMain, stubMainClipboard } from './l10-12/main-clipboard';

// In the real window with the real Core and Git; only the model is a local
// script: 复制 that copies (item 1), actions under the answer's line and beside the user's message
// and none under notes between tools (items 2, 3), the bar's fluid filling it (item 4), a framed
// search box's focus (item 6), and the 近期活动 card with the uncommitted lines (item 7).

const USER = '把计划写进 l10-12-plan.md';
const NOTE = Array.from({ length: 10 }, (_, line) => `- 第 ${line + 1} 步`).join('\n') + '\n';
const ANSWER = '已写好 **l10-12-plan.md**，共 10 步。';

/** A workspace that is a Git repository with one commit, Git set up for this test alone. */
async function repository() {
  // Core takes a Git test profile only from a TEMP folder named aura-git-test-*.
  const root = await mkdtemp(join(tmpdir(), 'aura-git-test-l10-12-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const global = join(root, 'global.cfg');
  const system = join(root, 'system.cfg');
  const profile = join(root, 'profile.json');
  await writeFile(
    global,
    '[credential]\nhelper =\n[user]\nname = Fixture\nemail = fixture@example.invalid\n[commit]\ngpgsign = false\n',
  );
  await writeFile(system, '');
  const image = process.env.AURA_GIT_TEST_BINARY ?? 'C:/Program Files/Git/cmd/git.exe';
  await writeFile(
    profile,
    JSON.stringify({ global_config: global, system_config: system, git_path: image }),
  );
  const environment = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith('GIT_')),
  );
  const git = (args: string[]) =>
    promisify(execFile)(image, args, {
      cwd: workspace,
      windowsHide: true,
      env: {
        ...environment,
        HOME: root,
        XDG_CONFIG_HOME: root,
        GIT_CONFIG_GLOBAL: global,
        GIT_CONFIG_SYSTEM: system,
        GIT_CONFIG_NOSYSTEM: '1',
        GIT_TERMINAL_PROMPT: '0',
      },
    });
  await git(['init', '--initial-branch=main']);
  await writeFile(join(workspace, 'README.md'), '# Demo\n');
  await git(['add', '--all']);
  await git(['commit', '-m', 'base']);
  return { root, workspace, profile };
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core/Git in TEMP; scripted loopback model only.
test('L10.12 copying works, actions sit where asked, the card sums the changes, fields and the bar look right', async ({}, info) => {
  test.setTimeout(300_000);
  const repo = await repository();
  await info.attach('isolated-data-root', { body: repo.root, contentType: 'text/plain' });
  const model = await writeProvider({ path: 'l10-12-plan.md', text: NOTE, finalText: ANSWER });
  const { app, page } = await launch(repo.root, { AURA_GIT_TEST_PROFILE: repo.profile });
  try {
    await expectSecureSurface(page);
    const setup = await armReadOnlyWrite(page, repo.workspace, model.url, {
      title: 'L10.12',
      text: USER,
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await page.locator('.session-row').first().click();
    await answer(page, session, 'allow_once');
    await finished(page, session);

    // Items 2 and 3: no actions under the note between tools; the answer's under the turn's line,
    // the last thing in the turn; the user's message has its own 复制.
    const turn = page.locator('.agent-turn').last();
    // A finished turn folds its process behind its status line, which opens it.
    const status = turn.locator('.agent-turn-toggle');
    await expect(status).toHaveAttribute('aria-expanded', 'false');
    await expect(turn.locator('.agent-interim-reply')).toHaveCount(0);
    await status.click();
    await expect(turn.locator('.agent-interim-reply')).toHaveCount(1);
    await expect(turn.locator('.agent-interim-reply button')).toHaveCount(0);
    const footer = turn.locator(':scope > .agent-turn-footer');
    await expect(footer.getByRole('button', { name: '复制', exact: true })).toBeVisible();
    const placed = await turn.evaluate((element) => {
      const report = element.querySelector('.agent-final-report')!.getBoundingClientRect();
      const foot = element.querySelector(':scope > .agent-turn-footer')!;
      return {
        last: element.lastElementChild === foot,
        below: foot.getBoundingClientRect().top >= report.bottom,
        line: getComputedStyle(foot).borderTopStyle,
        height: Math.round(foot.getBoundingClientRect().height),
      };
    });
    expect(placed).toMatchObject({ last: true, below: true, line: 'solid' });
    expect(placed.height).toBeLessThanOrEqual(40);

    // The owner: the note between tools sits too far from what is above and below it —
    // "上下距离分别改为当前的1/3". Measured between the words themselves.
    const spacing = await turn.evaluate((element) => {
      const words = (node: Element) => {
        const range = document.createRange();
        range.selectNodeContents(node);
        const rects = [...range.getClientRects()].filter((rect) => rect.height > 0);
        return {
          top: Math.min(...rects.map((r) => r.top)),
          bottom: Math.max(...rects.map((r) => r.bottom)),
        };
      };
      const status = words(element.querySelector('.agent-turn-status [role="status"]')!);
      const note = element.querySelector('.agent-interim-reply')!;
      const text = words(note);
      const next = words(
        note.nextElementSibling!.querySelector('button') ?? note.nextElementSibling!,
      );
      return {
        above: Math.round(text.top - status.bottom),
        below: Math.round(next.top - text.bottom),
      };
    });
    console.log(`L10.12 note spacing: ${JSON.stringify(spacing)}`);
    // Before: 19 above and 18 below; a third of that, a pixel or two either way.
    expect(spacing.above).toBeGreaterThanOrEqual(3);
    expect(spacing.above).toBeLessThanOrEqual(8);
    expect(spacing.below).toBeGreaterThanOrEqual(3);
    expect(spacing.below).toBeLessThanOrEqual(8);

    // Item 1: copying reaches the clipboard through Main (replaced here: the owner's clipboard is
    // never touched).
    await stubMainClipboard(app);
    await footer.getByRole('button', { name: '复制', exact: true }).click();
    await expect.poll(() => copiedThroughMain(app)).toEqual([ANSWER]);
    const message = page.locator('.agent-user-message').last();
    await message.hover();
    await message.getByRole('button', { name: '复制消息' }).click();
    await expect.poll(() => copiedThroughMain(app)).toEqual([ANSWER, USER]);
    await page.screenshot({ path: info.outputPath('l10-12-actions.png') });

    // Item 7: the card says what happened and sums the uncommitted lines, new files included.
    const card = page.getByRole('complementary', { name: '近期活动', includeHidden: true });
    const entry = page
      .locator('.conversation-header')
      .getByRole('button', { name: /展开近期活动/u });
    if (await entry.isVisible()) await entry.click();
    await expect(card).toBeVisible();
    await expect(card).toContainText('空闲');
    await expect(card).toContainText('1 个文件 · +10 −0');
    // In parts, the lines added green.
    await expect(
      card.getByRole('region', { name: '未提交改动' }).locator('.activity-added'),
    ).toHaveCSS('color', 'rgb(59, 118, 87)');
    await expect(card.getByRole('list', { name: '最近操作' })).toContainText('写入文件');
    await page.screenshot({ path: info.outputPath('l10-12-activity.png') });
    await card.getByRole('button', { name: '查看审查' }).click();
    const review = page.getByRole('region', { name: 'Git 审查', exact: true });
    await expect(review).toBeVisible();
    await expect(review.locator('.review-summary')).toContainText('1 个文件 · +10 −0');

    // With the sidebar open the conversation is narrow: the entry sits in its header, and the card
    // it opens closes on a click elsewhere.
    await expect(entry).toBeVisible();
    await entry.click();
    await expect(card).toBeVisible();
    await page.locator('.conversation-header h2').click();
    await expect(card).toBeHidden();

    // Item 6: a framed search box shows focus on its frame — the field itself draws nothing.
    await review.getByRole('button', { name: '搜索改动' }).click();
    const search = review.locator('.review-search-field');
    await expect(search).toBeVisible();
    const focused = await search.evaluate(async (frame) => {
      const field = frame.querySelector('input')!;
      field.focus();
      // The frame eases into its focus colour; read it once it has.
      await new Promise((done) => setTimeout(done, 400));
      const outer = getComputedStyle(frame);
      const inner = getComputedStyle(field);
      return {
        border: outer.borderTopColor,
        ring: outer.boxShadow,
        outline: inner.outlineStyle,
        shadow: inner.boxShadow,
      };
    });
    expect(focused).toMatchObject({ border: 'rgb(46, 105, 115)', outline: 'none', shadow: 'none' });
    expect(focused.ring).toContain('rgba(46, 105, 115, 0.15)');
    await page.screenshot({ path: info.outputPath('l10-12-search-focus.png') });

    // The context card shows its pixel rows inside its edge.
    await page.getByRole('button', { name: /^上下文占用/u }).click();
    const bar = page.getByRole('region', { name: '上下文占用详情' });
    await expect(bar.locator('.context-pixel-head')).toContainText('上下文');
    expect(await bar.evaluate((section) => section.scrollWidth <= section.clientWidth)).toBe(true);
    // The bar fades in; the picture waits for it.
    await page.waitForTimeout(400);
    await page.screenshot({ path: info.outputPath('l10-12-context-bar.png') });
    expect(model.errors).toEqual([]);
  } finally {
    await app.close();
    await model.close();
  }
});
