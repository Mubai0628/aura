import { expect, test, type ElectronApplication, type Locator, type Page } from '@playwright/test';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { launch } from './s06/harness';
import { writeProvider, type ProviderAction } from './s06-file-write-provider.mjs';
import { answered, send } from './l10-14/conversation';

// In the real window with the real Core; only the model
// is a local script. The collapsed 近期活动 card says how far the task list has got; the list's
// arrow and × stand in line; the two buttons at the top right stand in line; coming back from 设置
// shows the conversation at once, with every step named; turns out of sight are not laid out again
// while a divider is dragged; and an answer that arrives as one 600 KB event line still lands.

type Status = 'pending' | 'in_progress' | 'completed';
const todo = (items: [string, Status][]): ProviderAction => ({
  tool: 'todo.replace',
  arguments: {
    expected_revision: '1',
    items: items.map(([title, status]) => ({ title, status })),
  },
});

async function size(app: ElectronApplication, width: number, height: number) {
  await app.evaluate(
    ({ BrowserWindow }, [width, height]) => {
      BrowserWindow.getAllWindows()[0]!.setSize(width!, height!);
    },
    [width, height],
  );
}

/** The 近期活动 card, opened the way a user does when it waits behind its entry. */
async function openActivity(page: Page) {
  const entry = page.getByRole('button', { name: /^展开近期活动/u });
  if (await entry.isVisible()) await entry.click();
  const card = page.getByRole('complementary', { name: '近期活动' });
  await expect(card).toBeVisible();
  return card;
}

/** A box once it has settled: a card still opening is drawn under a running scale. */
async function settled(locator: Locator, width: number) {
  await expect
    .poll(async () => Math.round((await locator.boundingBox())?.width ?? 0), { timeout: 5_000 })
    .toBe(width);
  return box(locator);
}

async function box(locator: Locator) {
  const found = await locator.boundingBox();
  expect(found).not.toBeNull();
  return found!;
}

const middle = (rect: { y: number; height: number }) => rect.y + rect.height / 2;

// eslint-disable-next-line no-empty-pattern -- real Electron/Core in TEMP; scripted loopback model only.
test('L10.18 the card and header buttons line up, the collapsed card counts, coming back shows at once, and one long event line lands', async ({}, info) => {
  test.setTimeout(480_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l10-18-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace, { recursive: true });
  const order = new Map<string, number>();
  const model = await writeProvider({
    path: 'unused.md',
    text: '',
    finalText: '好了。',
    // The first turn writes a list with one item done; the second finishes it; later turns answer.
    planAction: (request) => {
      if (!order.has(request.turn)) order.set(request.turn, order.size);
      const turn = order.get(request.turn);
      if (request.step > 0 || turn! > 1) return { tool: null, arguments: {} };
      return turn === 0
        ? todo([
            ['读需求', 'completed'],
            ['写代码', 'in_progress'],
            ['测试', 'pending'],
          ])
        : todo([
            ['读需求', 'completed'],
            ['写代码', 'completed'],
            ['测试', 'completed'],
          ]);
    },
  });
  const { app, page } = await launch(root);
  try {
    await size(app, 1400, 860);
    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L10.18',
      text: '列个清单',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const first = page.locator('.session-row').first();
    await first.click();
    await answered(page, session, 1);

    // Decision 2: beside the open tool panel the card waits behind its entry, which still says
    // how far the list has got.
    const entry = page.getByRole('button', { name: /^展开近期活动/u });
    await expect(entry).toBeVisible({ timeout: 30_000 });
    await expect(entry).toHaveAccessibleName(/待办 1\/3/u);
    await expect(entry).toContainText('待办 1/3');
    await entry.screenshot({ path: info.outputPath('collapsed-entry.png') });

    // Item 6: the list's arrow is an icon square on the heading's line.
    const card = await openActivity(page);
    const list = card.locator('section.todo-dock');
    const fold = list.getByRole('button', { name: '收起清单' });
    const foldBox = await settled(fold, 22);
    expect(Math.round(foldBox.height)).toBe(22);
    expect(await fold.evaluate((button) => getComputedStyle(button).borderTopWidth)).toBe('0px');
    const count = await box(list.getByLabel('已完成 1 / 3'));
    expect(Math.abs(middle(count) - middle(foldBox))).toBeLessThanOrEqual(1.5);
    await card.screenshot({ path: info.outputPath('card-list-under-way.png') });

    // Item 5: back from 设置 the conversation is there at once, its step named, nothing loading.
    // A finished turn is folded; opened, it shows its process line.
    await page.getByTitle('展开过程').first().click();
    const label = page.locator('.agent-process-label').first();
    await expect(label).toBeVisible({ timeout: 30_000 });
    await expect(label).not.toHaveText('工具调用', { timeout: 30_000 });
    const named = (await label.textContent()) ?? '';
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: '设置' })).toBeVisible();
    await page.getByRole('button', { name: '返回会话', exact: true }).click();
    await expect(label).toHaveText(named, { timeout: 1_000 });
    const loading = page.getByText('正在加载会话…');
    for (let check = 0; check < 10; check++) {
      expect(await loading.count()).toBe(0);
      await page.waitForTimeout(100);
    }

    // Item 6 (stutter): a turn out of sight keeps its last size while a divider is dragged.
    expect(
      await page
        .locator('.agent-turn')
        .first()
        .evaluate((turn) => getComputedStyle(turn).contentVisibility),
    ).toBe('auto');

    // Item 3 (DeepSeek): the answer arrives as one event line of 600 KB, more than the old
    // 256 KiB limit; the turn completes instead of stopping with "任务暂时中断".
    model.setFinalText('x'.repeat(600_000));
    await send(page, session, '写长一点');
    await answered(page, session, 2);

    // Item 1: with every item done, the arrow and the × stand side by side on one line.
    const done = await openActivity(page);
    const doneList = done.locator('section.todo-dock');
    await expect(doneList.getByLabel('已完成 3 / 3')).toBeVisible({ timeout: 30_000 });
    const arrow = await settled(doneList.getByRole('button', { name: '收起清单' }), 22);
    const close = await settled(doneList.getByRole('button', { name: '关闭清单' }), 22);
    expect(Math.round(close.height)).toBe(22);
    expect(Math.abs(arrow.y - close.y)).toBeLessThanOrEqual(0.5);
    await done.screenshot({ path: info.outputPath('card-list-done.png') });

    // Item 9: with the right sidebar closed, the folder and the reopen button share one line and
    // one size, one header gap apart.
    await page.getByRole('button', { name: '关闭右侧边栏' }).click();
    // The pane closes with a short slide: measured once the conversation spans the whole width.
    await expect
      .poll(async () => {
        const header = await box(page.locator('.conversation-header'));
        const main = await box(page.locator('main.workspace-main'));
        return Math.round(main.x + main.width - (header.x + header.width));
      })
      .toBe(0);
    const folder = await box(page.getByRole('button', { name: '打开文件工具区' }));
    const reopen = await box(page.getByRole('button', { name: '展开右侧边栏' }));
    expect(Math.round(folder.width)).toBe(Math.round(reopen.width));
    expect(Math.round(folder.height)).toBe(Math.round(reopen.height));
    expect(Math.abs(middle(folder) - middle(reopen))).toBeLessThanOrEqual(1);
    expect(Math.round(reopen.x - (folder.x + folder.width))).toBe(7);
    await page
      .locator('.conversation-header')
      .screenshot({ path: info.outputPath('header-buttons.png') });
  } finally {
    await app.close();
    await model.close();
  }
});
