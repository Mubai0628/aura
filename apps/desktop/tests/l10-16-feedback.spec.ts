import { expect, test, type ElectronApplication, type Page } from '@playwright/test';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { launch } from './s06/harness';
import { writeProvider, type ProviderAction } from './s06-file-write-provider.mjs';
import { answered, send } from './l10-14/conversation';

// In the real window with the real Core; only the model is a local script.
// The startup screen is rain on a lake; an empty conversation has sharks swimming under the
// composer; the Todo card counts what is done, folds, and can be closed once everything is; a
// conversation with a turn under way shows it in the sidebar; and the thinking level is the
// conversation's own, from the levels its model offers.

type Status = 'pending' | 'in_progress' | 'completed';
const todo = (items: [string, Status][]): ProviderAction => ({
  tool: 'todo.replace',
  arguments: {
    expected_revision: '1',
    items: items.map(([title, status]) => ({ title, status })),
  },
});
const STEPS = ['读需求', '写代码', '测试'];

async function size(app: ElectronApplication, width: number, height: number) {
  await app.evaluate(
    ({ BrowserWindow }, [width, height]) => {
      BrowserWindow.getAllWindows()[0]!.setSize(width!, height!);
    },
    [width, height],
  );
}

/** How many of a canvas's sampled pixels are drawn on; `-1` once the canvas is gone. */
async function drawn(page: Page, selector: string) {
  return page.evaluate((selector) => {
    const canvas = document.querySelector(selector);
    if (!(canvas instanceof HTMLCanvasElement)) return -1;
    const pixels = canvas.getContext('2d')!.getImageData(0, 0, canvas.width, canvas.height).data;
    let count = 0;
    for (let i = 3; i < pixels.length; i += 4 * 7) if (pixels[i]! > 0) count += 1;
    return count;
  }, selector);
}

/**
 * The Todo list is a part of the 近期活动 card. Beside an open tool panel the window is too
 * narrow for the card, which then waits behind its entry; this opens it the way a user does.
 */
async function openActivity(page: Page) {
  const entry = page.getByRole('button', { name: /^展开近期活动/u });
  if (await entry.isVisible()) await entry.click();
  const card = page.getByRole('complementary', { name: '近期活动' });
  await expect(card).toBeVisible();
  return card;
}

/** Sends `text` the way the user does: typed into the composer and sent with Enter. */
async function type(page: Page, text: string) {
  const box = page.getByRole('textbox', { name: '输入消息' });
  await box.click();
  await box.fill(text);
  await box.press('Enter');
}

/** A second conversation in the workspace, on the same model. */
async function secondSession(page: Page, workspace: string, profile: string) {
  const made = await page.evaluate(
    async ({ workspace, profile }) => {
      const api = window.aura.v1;
      const session = await api.createSession({ workspace_id: workspace, title: '第二个会话' });
      if (!session.ok) return { error: JSON.stringify(session.error) };
      const binding = await api.setSessionModelBinding({
        session_id: session.value.session_id,
        expected_revision: '0',
        model_profile_id: profile,
      });
      if (!binding.ok) return { error: JSON.stringify(binding.error) };
      return { session: session.value.session_id };
    },
    { workspace, profile },
  );
  expect(made.error).toBeUndefined();
  return made.session as string;
}

/** The model lists every level it takes, the way a user ticks them in its settings. */
async function offerEveryLevel(page: Page, profile: string) {
  const updated = await page.evaluate(async (profile) => {
    const api = window.aura.v1;
    const settings = await api.getModelSettings();
    if (!settings.ok) return 'settings';
    const model = settings.value.registry.models[profile];
    if (!model) return 'model';
    const result = await api.updateModelProfile({
      expected_revision: settings.value.models_revision,
      model_profile_id: profile,
      display_name: model.display_name,
      notes: model.notes ?? null,
      enabled: true,
      request_defaults: {
        ...model.request_defaults,
        reasoning_efforts: ['low', 'medium', 'high', 'xhigh', 'max'],
      },
      historical_snapshot_max_tokens: model.context.historical_snapshot_max_tokens,
    });
    return result.ok ? 'ok' : JSON.stringify(result.error);
  }, profile);
  expect(updated).toBe('ok');
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core in TEMP; scripted loopback model only.
test('L10.16 rain at startup, sharks in an empty conversation, the Todo card, a running conversation and its own thinking level', async ({}, info) => {
  test.setTimeout(420_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l10-16-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace, { recursive: true });
  const bodies: Record<string, unknown>[] = [];
  const order = new Map<string, number>();
  const model = await writeProvider({
    path: 'unused.md',
    text: '',
    finalText: '好了。',
    // The first turn writes the list with one item done and one under way; the second finishes
    // it; every later turn just answers.
    planAction: (request) => {
      if (!order.has(request.turn)) order.set(request.turn, order.size);
      if (request.step > 0) return { tool: null, arguments: {} };
      const turn = order.get(request.turn);
      if (turn === 0)
        return todo([
          [STEPS[0]!, 'completed'],
          [STEPS[1]!, 'in_progress'],
          [STEPS[2]!, 'pending'],
        ]);
      if (turn === 1) return todo(STEPS.map((step) => [step, 'completed']));
      return { tool: null, arguments: {} };
    },
    observeRequest: ({ body }) => bodies.push(body as Record<string, unknown>),
  });
  const { app, page } = await launch(root);
  try {
    await size(app, 1400, 860);

    // Item 3: the startup screen is rain on a lake with the name over it, and it leaves once
    // Core is ready.
    await page.reload();
    await expect
      .poll(() => drawn(page, '#aura-splash-lake'), { intervals: [50], timeout: 10_000 })
      .toBeGreaterThanOrEqual(400);
    await page.screenshot({ path: info.outputPath('startup-rain.png') });
    await expect(page.locator('#aura-splash')).toHaveCount(0, { timeout: 30_000 });
    await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 60_000 });

    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L10.16',
      text: '列个清单',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const profile = setup.profileId as string;
    // The first conversation, whatever title it takes once the composer has sent in it.
    const first = page.locator('.session-row').filter({ hasNotText: '第二个会话' }).first();
    await first.click();
    await answered(page, session, 1);

    // Item 1: the card counts what is done, the item under way turns, and it folds to that item.
    const card = (await openActivity(page)).locator('section.todo-dock');
    await expect(card).toBeVisible({ timeout: 30_000 });
    await expect(card.getByLabel('已完成 1 / 3')).toBeVisible();
    await expect(card.locator('.todo-item.is-in_progress')).toContainText(STEPS[1]!);
    await expect(card.getByRole('button', { name: '关闭清单' })).toHaveCount(0);
    await card.screenshot({ path: info.outputPath('todo-under-way.png') });
    await card.getByRole('button', { name: '收起清单' }).click();
    await expect(card).toHaveAttribute('data-folded', 'true');
    await expect(card.locator('.todo-head-current')).toContainText(STEPS[1]!);
    await card.screenshot({ path: info.outputPath('todo-folded.png') });
    await card.getByRole('button', { name: '展开清单' }).click();
    await expect(card).not.toHaveAttribute('data-folded', 'true');

    // Item 4: while a turn is under way its conversation shows it in the sidebar.
    const release = model.holdNext();
    await send(page, session, '都做完');
    const dot = first.locator('.session-state');
    await expect(dot).toHaveClass(/is-running/u, { timeout: 15_000 });
    await expect(first).toContainText('正在运行');
    await page
      .getByRole('complementary', { name: '工作区导航' })
      .screenshot({ path: info.outputPath('sidebar-running.png') });
    release();
    await answered(page, session, 2);
    await expect(dot).not.toHaveClass(/is-running/u, { timeout: 15_000 });

    // Item 1: once everything is done the card can be closed, and it stays closed.
    await expect(card.getByLabel('已完成 3 / 3')).toBeVisible({ timeout: 30_000 });
    await card.screenshot({ path: info.outputPath('todo-done.png') });
    await card.getByRole('button', { name: '关闭清单' }).click();
    await expect(card).toHaveCount(0);
    await page.reload();
    await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 60_000 });
    await first.click();
    await expect(page.getByRole('textbox', { name: '输入消息' })).toBeVisible();
    // Not in the page at all, whether the card around it is open or waiting behind its entry.
    await expect(page.locator('section.todo-dock')).toHaveCount(0);

    // Item 2: the model offers every level up to 最高; the conversation's choice goes with its
    // requests, and another conversation keeps the model's default.
    await offerEveryLevel(page, profile);
    const thinking = page.getByRole('button', { name: '思考程度：默认' });
    await expect(thinking).toBeVisible({ timeout: 30_000 });
    await thinking.click();
    const levels = page.getByRole('menu', { name: '思考程度' });
    await expect(levels.getByRole('menuitemradio')).toHaveText([
      '默认',
      '低',
      '中',
      '高',
      '超高',
      '最高',
    ]);
    await levels.screenshot({ path: info.outputPath('thinking-levels.png') });
    await levels.getByRole('menuitemradio', { name: '最高' }).click();
    await expect(page.getByRole('button', { name: '思考程度：最高' })).toBeVisible();
    await type(page, '用最高档');
    await answered(page, session, 3);
    expect(bodies.at(-1)).toHaveProperty('reasoning_effort', 'max');

    const other = await secondSession(page, setup.workspaceId as string, profile);
    await page.locator('.session-row', { hasText: '第二个会话' }).click();
    await expect(page.getByRole('button', { name: '思考程度：默认' })).toBeVisible({
      timeout: 30_000,
    });
    await type(page, '用默认档');
    await answered(page, other, 1);
    expect(bodies.at(-1)).not.toHaveProperty('reasoning_effort');
    await first.click();
    await expect(page.getByRole('button', { name: '思考程度：最高' })).toBeVisible({
      timeout: 30_000,
    });

    // Item 3: an empty conversation has sharks swimming under the composer.
    await page.getByRole('button', { name: '在 L10.16 新建会话' }).click();
    const sea = page.locator('.ocean-backdrop canvas');
    await expect(sea).toBeVisible({ timeout: 30_000 });
    await expect.poll(() => drawn(page, '.ocean-backdrop canvas')).toBeGreaterThan(400);
    await page.getByRole('textbox', { name: '输入消息' }).click();
    await page.screenshot({ path: info.outputPath('empty-conversation.png') });
  } finally {
    await app.close();
    await model.close();
  }
});
