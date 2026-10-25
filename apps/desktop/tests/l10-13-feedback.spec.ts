import { expect, test, type ElectronApplication, type Page } from '@playwright/test';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { answer, history, launch } from './s06/harness';
import {
  writeProvider,
  type ProviderAction,
  type ProviderRequest,
} from './s06-file-write-provider.mjs';

// In the real window with the real Core; only the model is a local script.
// The user's message sits on the right, as wide as its words; a finished turn's process folds
// behind its status line; switching conversations keeps the permission readable with no failure
// notice and the composer's words in place, also while a task runs; a branch opens quickly.

const FIRST = '把说明写进 l10-13-note.md';
const SHORT = '好';
const LONG = '请把这一段整理成三条要点，保留原文里的文件名与命令，不要改动代码。'.repeat(5);
const NOTICE = /权限与沙箱操作未完成|权限状态暂时读不到/u;

/** The conversation's last turn has answered and nothing runs. */
async function answered(page: Page, session: string, turns: number) {
  await expect
    .poll(
      async () => {
        const view = await history(page, session);
        return !view.active && view.turns.length === turns
          ? view.turns.at(-1)?.turn.state.state
          : undefined;
      },
      { timeout: 90_000 },
    )
    .toBe('completed');
}

async function send(page: Page, session: string, text: string) {
  const sent = await page.evaluate(
    async ({ session_id, text }) =>
      window.aura.v1.submitAgentMessage({ session_id, text, destination: { lane: 'next_turn' } }),
    { session_id: session, text },
  );
  expect(sent.ok, JSON.stringify(sent)).toBe(true);
}

async function width(app: ElectronApplication, pixels: number) {
  await app.evaluate(({ BrowserWindow }, pixels) => {
    const window = BrowserWindow.getAllWindows()[0]!;
    const [, height] = window.getSize();
    window.setSize(pixels, Math.max(height!, 820));
  }, pixels);
}

/** The composer shows the permission and model words, not their icons alone. */
async function wordsShown(page: Page) {
  const row = page.locator('.composer-controls');
  await expect(row).not.toHaveAttribute('data-compact', 'true');
  await expect(row.locator('.composer-select').first()).toContainText(/只读|工作区读写|完全访问/u);
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core in TEMP; scripted loopback model only.
test('L10.13 bubbles fit their words, a finished process folds, switching keeps the permission and the composer words', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l10-13-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  // The first turn writes the note; every later turn just answers.
  let first: string | undefined;
  const model = await writeProvider({
    path: 'l10-13-note.md',
    text: '说明\n',
    finalText: '好的，已经记下。',
    planAction: (request: ProviderRequest, action: ProviderAction): ProviderAction =>
      (first ??= request.turn) === request.turn && request.step === 0
        ? action
        : { tool: null, arguments: {} },
  });
  const { app, page } = await launch(root);
  try {
    await width(app, 1400);
    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L10.13',
      text: FIRST,
    });
    expect(setup.error).toBeUndefined();
    const source = setup.sessionId as string;
    await page.locator('.session-row').first().click();
    await answer(page, source, 'allow_once');
    await answered(page, source, 1);

    // Item 4: "过程部分结束后自动折叠起来，也能手动打开和折叠".
    const turn = page.locator('.agent-turn').first();
    const status = turn.locator('.agent-turn-toggle');
    await expect(status).toHaveAttribute('aria-expanded', 'false');
    await expect(turn.locator('.agent-process-section')).toHaveCount(0);
    await expect(turn.locator('.agent-final-report')).toBeVisible();
    await status.click();
    await expect(status).toHaveAttribute('aria-expanded', 'true');
    await expect(turn.locator('.agent-process-section').first()).toBeVisible();
    await page.screenshot({ path: info.outputPath('process-open.png') });
    await status.click();
    await expect(turn.locator('.agent-process-section')).toHaveCount(0);

    // Item 2: on the right, as wide as its words up to the width it always had; the words start
    // on the left.
    await send(page, source, SHORT);
    await answered(page, source, 2);
    await send(page, source, LONG);
    await answered(page, source, 3);
    const bubbles = await page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>('.agent-turn')].map((element) => {
        const bubble = element.querySelector<HTMLElement>(
          '.agent-user-message:not(.agent-steer-message)',
        )!;
        const outer = element.getBoundingClientRect();
        const box = bubble.getBoundingClientRect();
        const words = document.createRange();
        words.selectNodeContents(bubble.querySelector('p') ?? bubble);
        const lines = [...words.getClientRects()].filter((rect) => rect.width > 0);
        return {
          share: box.width / outer.width,
          width: Math.round(box.width),
          right: Math.round(outer.right - box.right),
          starts: [...new Set(lines.map((rect) => Math.round(rect.left - box.left)))],
          lines: new Set(lines.map((rect) => Math.round(rect.top))).size,
        };
      }),
    );
    console.log(`L10.13 bubbles: ${JSON.stringify(bubbles)}`);
    expect(bubbles).toHaveLength(3);
    for (const bubble of bubbles) {
      expect(bubble.right).toBeLessThanOrEqual(1);
      // Every line starts at the bubble's left padding.
      expect(bubble.starts).toHaveLength(1);
      expect(bubble.starts[0]).toBeGreaterThanOrEqual(14);
      expect(bubble.starts[0]).toBeLessThanOrEqual(18);
    }
    expect(bubbles[0]!.share).toBeLessThan(0.6);
    expect(bubbles[1]!.width).toBeLessThan(80);
    expect(bubbles[2]!.lines).toBeGreaterThan(1);
    expect(bubbles[2]!.share).toBeGreaterThan(0.88);
    expect(bubbles[2]!.share).toBeLessThanOrEqual(0.921);
    await page.screenshot({ path: info.outputPath('bubbles.png') });

    // Item 5: the words stay while a task runs (its stop button's ring turns at the row's end) and
    // while the context bar floats above the row.
    await wordsShown(page);
    const release = model.holdNext();
    await send(page, source, '再说一次');
    await expect(page.getByRole('button', { name: '停止当前任务' })).toBeVisible();
    for (let look = 0; look < 6; look += 1) {
      await wordsShown(page);
      await page.waitForTimeout(200);
    }
    release();
    await answered(page, source, 4);
    await wordsShown(page);
    const ring = page.getByRole('button', { name: /^上下文占用/u });
    await ring.click();
    await expect(page.getByRole('region', { name: '上下文占用详情' })).toBeVisible();
    await wordsShown(page);
    await ring.click();
    // In a narrower window the model's whole name shows while it has room (no fixed 95px cut).
    await width(app, 1100);
    await wordsShown(page);
    const name = page.locator('.model-select [data-adaptive-label]');
    expect(
      await name.evaluate((label) => label.scrollWidth - label.clientWidth),
    ).toBeLessThanOrEqual(0);
    await width(app, 1400);

    // Item 1: switching conversations reads the permission every time, without a failure notice.
    for (const title of ['第二个', '第三个']) {
      const made = await page.evaluate(
        async ({ workspace_id, title }) => window.aura.v1.createSession({ workspace_id, title }),
        { workspace_id: setup.workspaceId as string, title },
      );
      expect(made.ok, JSON.stringify(made)).toBe(true);
    }
    const rows = page.locator('.session-row');
    await expect(rows).toHaveCount(3);
    for (let round = 0; round < 12; round += 1) {
      await rows.nth(round % 3).click();
      await wordsShown(page);
      await expect(page.locator('.notification-viewport').getByText(NOTICE)).toHaveCount(0);
    }

    // Item 1: a branch of this conversation opens quickly. Started through the API, it keeps an
    // empty title, which the sidebar shows as 新会话.
    await rows.filter({ hasText: '新会话' }).first().click();
    await expect(page.locator('.agent-turn')).toHaveCount(4);
    const footer = page.locator('.agent-turn').last().locator(':scope > .agent-turn-footer');
    await footer.getByRole('button', { name: '新建分支' }).click();
    const started = Date.now();
    await page.getByRole('dialog').getByRole('button', { name: '新建分支' }).click();
    await expect(page.getByRole('note', { name: '分支起点' })).toBeVisible({ timeout: 30_000 });
    const opened = Date.now() - started;
    console.log(`L10.13 branch of 4 turns opened after ${opened} ms`);
    expect(opened).toBeLessThan(5_000);
    await wordsShown(page);
    await expect(page.locator('.notification-viewport').getByText(NOTICE)).toHaveCount(0);
    expect(model.errors).toEqual([]);
  } finally {
    await app.close();
    await model.close();
  }
});
