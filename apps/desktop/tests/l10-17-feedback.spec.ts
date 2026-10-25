import { expect, test, type ElectronApplication, type Locator, type Page } from '@playwright/test';
import { mkdir, mkdtemp, readFile, readdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { launch } from './s06/harness';
import { writeProvider, type ProviderAction } from './s06-file-write-provider.mjs';
import { answered, send } from './l10-14/conversation';

// In the real window with the real Core; only the model
// is a local script. The name rolls in on reels over the rain with nothing behind it; the Todo list
// is a part of the 近期活动 card; the thinking count goes on across switching conversations; a model
// whose answer starts only after the connect limit is still waited for; a model's thinking levels
// are pills and its default one choice on a segmented track; and once Main has said it is quitting,
// the next start checks only the store's structure and the rest in the background.

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

/** The seconds the "thinking" line counts, or -1 while it is not shown. */
async function thinkingSeconds(page: Page) {
  const line = page.getByRole('status').filter({ hasText: '正在思考下一步' });
  if (!(await line.isVisible())) return -1;
  const matched = /(\d+) 秒/u.exec((await line.textContent()) ?? '');
  return matched ? Number(matched[1]) : -1;
}

/** Every Core log entry named `event`, oldest first. */
async function coreLog(root: string, event: string) {
  const text = await readFile(join(root, 'Local', 'Aura', 'logs', 'core.jsonl'), 'utf8').catch(
    () => '',
  );
  return text
    .split('\n')
    .filter((line) => line.includes(`"event":"${event}"`))
    .map((line) => JSON.parse(line) as { fields: Record<string, unknown> });
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core in TEMP; scripted loopback model only.
test('L10.17 reels at startup, the Todo in the activity card, the thinking count, a slow model, the level controls and a normal quit', async ({}, info) => {
  test.setTimeout(480_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l10-17-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace, { recursive: true });
  const order = new Map<string, number>();
  const model = await writeProvider({
    path: 'unused.md',
    text: '',
    finalText: '好了。',
    // The first turn writes a list; every later turn just answers.
    planAction: (request) => {
      if (!order.has(request.turn)) order.set(request.turn, order.size);
      if (request.step > 0 || order.get(request.turn) !== 0) return { tool: null, arguments: {} };
      return todo([
        ['读需求', 'completed'],
        ['写代码', 'in_progress'],
        ['测试', 'pending'],
      ]);
    },
  });
  let { app, page } = await launch(root);
  try {
    await size(app, 1400, 860);

    // Item 1: the reels turn from the screen's first frame, and the screen leaves only once they
    // have stopped.
    await page.reload();
    let introEnds = Number.NaN;
    await expect
      .poll(
        async () => {
          introEnds = await page.evaluate(() =>
            Number(document.getElementById('aura-splash')?.dataset.introEnds),
          );
          return Number.isFinite(introEnds);
        },
        { intervals: [20], timeout: 10_000 },
      )
      .toBe(true);
    for (let shot = 0; shot < 6; shot++)
      await page.screenshot({ path: info.outputPath(`startup-${shot}.png`) });
    let leftAt = Number.NaN;
    await expect
      .poll(
        async () => {
          leftAt = await page.evaluate(() =>
            document.querySelector('#aura-splash.is-leaving') ||
            !document.getElementById('aura-splash')
              ? performance.now()
              : Number.NaN,
          );
          return Number.isFinite(leftAt);
        },
        { intervals: [20], timeout: 30_000 },
      )
      .toBe(true);
    expect(leftAt).toBeGreaterThanOrEqual(introEnds - 5);
    await expect(page.locator('#aura-splash')).toHaveCount(0, { timeout: 30_000 });
    await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 60_000 });

    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L10.17',
      text: '列个清单',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const profile = setup.profileId as string;
    const first = page.locator('.session-row').filter({ hasNotText: '第二个会话' }).first();
    await first.click();
    await answered(page, session, 1);

    // Item 6: the list is a part of the 近期活动 card, no longer above the composer.
    const activity = await openActivity(page);
    const list = activity.locator('section.todo-dock');
    await expect(list).toBeVisible({ timeout: 30_000 });
    await expect(list.getByRole('heading', { name: '待办' })).toBeVisible();
    await expect(list.getByLabel('已完成 1 / 3')).toBeVisible();
    await expect(page.locator('.conversation-dock-inner section.todo-dock')).toHaveCount(0);
    await activity.screenshot({ path: info.outputPath('activity-with-todo.png') });

    // Item 4: the model has said something and gone quiet; switching away and back goes on
    // counting instead of starting again.
    await secondSession(page, setup.workspaceId as string, profile);
    const release = model.holdNext('先想想。');
    await send(page, session, '想一想');
    await expect.poll(() => thinkingSeconds(page), { timeout: 30_000 }).toBeGreaterThanOrEqual(3);
    const before = await thinkingSeconds(page);
    await page.locator('.session-row', { hasText: '第二个会话' }).click();
    await expect(page.getByRole('textbox', { name: '输入消息' })).toBeVisible();
    await page.waitForTimeout(3_000);
    await first.click();
    await expect
      .poll(() => thinkingSeconds(page), { intervals: [100], timeout: 1_500 })
      .toBeGreaterThanOrEqual(before + 3);
    await page.screenshot({ path: info.outputPath('thinking-count.png') });
    release();
    await answered(page, session, 2);

    // Item 5: the answer's headers come after the endpoint's connect limit (5 s) but within its
    // first-event limit (10 s): one request, waited for, and the turn completes.
    const requests = model.requests.length;
    model.delayHeadersNext(7_000);
    await send(page, session, '慢一点回答');
    await answered(page, session, 3);
    expect(model.requests.length - requests).toBe(1);

    // Item 3: the model's levels are pills and its default one choice on a segmented track.
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await page
      .locator('.settings-navigation')
      .getByRole('button', { name: '模型', exact: true })
      .click();
    const card = page.locator('article.model-card', { hasText: 'S06 fixture model' });
    if (!(await card.isVisible().catch(() => false)))
      await page.getByText('S06 isolated model').first().click();
    await card.getByRole('button', { name: '编辑', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '编辑模型' });
    await expect(dialog).toBeVisible();
    const levels = dialog.getByRole('group', { name: '可选思考档位' });
    const choice = dialog.getByRole('radiogroup', { name: '会话默认思考程度' });
    const named = (radios: Locator) =>
      radios.evaluateAll((all) => all.map((radio) => radio.closest('label')?.textContent));
    // The model's format offers 低, 中 and 高 until its own list is set; the default comes first.
    await expect
      .poll(() => named(levels.getByRole('checkbox', { checked: true })))
      .toEqual(['低', '中', '高']);
    await expect
      .poll(() => named(choice.getByRole('radio')))
      .toEqual(['官方默认', '低', '中', '高']);
    // The real boxes lie over the whole of each pill and segment: a click on one is a click on it.
    await levels.getByRole('checkbox', { name: '低', exact: true }).click();
    await levels.getByRole('checkbox', { name: '最高', exact: true }).click();
    await expect
      .poll(() => named(choice.getByRole('radio')))
      .toEqual(['官方默认', '中', '高', '最高']);
    await choice.getByRole('radio', { name: '最高', exact: true }).click();
    await expect(choice.getByRole('radio', { name: '最高', exact: true })).toBeChecked();
    // A pill is one row: its tick and its name side by side, no taller than the segments.
    const pill = await levels.locator('label').first().boundingBox();
    const segment = await choice.locator('label').first().boundingBox();
    expect(pill!.height).toBeLessThanOrEqual(32);
    expect(segment!.height).toBeLessThanOrEqual(32);
    await dialog.screenshot({ path: info.outputPath('model-dialog.png') });
    await dialog.getByRole('button', { name: '保存 Profile' }).click();
    await expect(dialog).toHaveCount(0);
    const saved = await page.evaluate(async (profile) => {
      const settings = await window.aura.v1.getModelSettings();
      return settings.ok ? settings.value.registry.models[profile]?.request_defaults : undefined;
    }, profile);
    expect(saved?.reasoning_effort).toBe('max');
    expect(saved?.reasoning_efforts).toEqual(['medium', 'high', 'max']);

    // Item 2: Main says it is quitting before it stops Core; Core records the normal exit, and
    // the next start checks only the store's structure, then every page in the background.
    const exited = new Promise((done) => app.process().once('exit', done));
    await app.evaluate(({ app }) => app.quit()).catch(() => undefined);
    await exited;
    expect((await coreLog(root, 'store_clean_exit')).at(-1)?.fields.marked).toBe(true);
    const files = await readdir(root, { recursive: true });
    expect(files.filter((file) => file.endsWith('clean-exit.json'))).toHaveLength(1);
    ({ app, page } = await launch(root));
    expect((await coreLog(root, 'store_open_check')).at(-1)?.fields.check).toBe('structure');
    await expect
      .poll(async () => (await coreLog(root, 'store_quick_check')).at(-1)?.fields.ok, {
        timeout: 30_000,
      })
      .toBe(true);
    expect(
      (await readdir(root, { recursive: true })).filter((file) => file.endsWith('clean-exit.json')),
      'the record is spent as the store opens',
    ).toHaveLength(0);
  } finally {
    await app.close().catch(() => undefined);
    await model.close();
  }
});
