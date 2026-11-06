import { expect, test, type Page } from '@playwright/test';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armOwnerRound, failures, mainEvents } from './l18-11/arm';
import { pixelAudit, type PixelFinding } from './pixel/audit';
import { launch } from './s06/harness';

// In the real window with the real Core. Core answers a read it has no room for with a refusal;
// a burst of them once overflowed the answers' reserved room and Core dropped the connection, so
// every read in flight failed and the settings pages read nothing until reopened. Provider icons
// were that burst: the catalog asked for all of them at once.

/** Catalog reads at once: well past the eight Core runs and the 32 it queues per connection. */
const BURST = 160;

/** The pixel audit, given the page a few moments to settle. */
async function settled(page: Page) {
  let findings: PixelFinding[] = await pixelAudit(page);
  for (let tries = 0; findings.length > 0 && tries < 10; tries += 1) {
    await page.waitForTimeout(500);
    findings = await pixelAudit(page);
  }
  return findings;
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core under the production policy, isolated data root.
test('L18.11 a burst Core cannot take is refused read by read and the connection stays', async ({}, info) => {
  test.setTimeout(240_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l18-11-burst-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const { app, page } = await launch(root);
  try {
    // Each read answers with the whole provider catalog, so Core's writer is still sending the
    // first answers while its read-only lane fills and the rest are refused.
    const burst = await page.evaluate(async (count) => {
      const started = performance.now();
      const replies = await Promise.all(
        Array.from({ length: count }, () => window.aura.v1.getModelProviderCatalog()),
      );
      const codes: Record<string, number> = {};
      for (const reply of replies) {
        const code = reply.ok ? 'ok' : String(reply.error.code);
        codes[code] = (codes[code] ?? 0) + 1;
      }
      return { codes, ms: Math.round(performance.now() - started) };
    }, BURST);
    await info.attach('burst', { body: JSON.stringify(burst), contentType: 'application/json' });
    console.log(`L18.11 burst of ${BURST}: ${JSON.stringify(burst)}`);
    expect(burst.codes['ipc.handler_overloaded'] ?? 0).toBeGreaterThan(0);
    expect(Object.keys(burst.codes).sort()).toEqual(['ipc.handler_overloaded', 'ok']);
    // The same connection answers what comes next.
    const after = await page.evaluate(() => window.aura.v1.getPermissionDefaults());
    expect(after.ok, JSON.stringify(after)).toBe(true);
    const events = await mainEvents(root);
    expect(events.filter(({ event }) => event.startsWith('core_transport'))).toEqual([]);
  } finally {
    await app.close();
  }
});

// eslint-disable-next-line no-empty-pattern -- real Electron/Core in TEMP; no model is asked anything.
test('L18.11 icons on demand, settings that read again, the switch, the session tile and the model filter', async ({}, info) => {
  test.setTimeout(240_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l18-11-owner-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace, { recursive: true });
  const { app, page } = await launch(root);
  try {
    const armed = await armOwnerRound(page, workspace);
    expect(armed).not.toHaveProperty('error');

    // The open conversation is one tile: its title and its 更多 button, hovered or not.
    await page.locator('.session-row').first().click();
    const item = page.locator('.session-nav-item:has([aria-current="page"])');
    await expect(item).toHaveCount(1);
    const more = item.locator('.sidebar-item-menu .icon-button');
    const tile = async () =>
      item.evaluate((li) => {
        const row = li.querySelector('.session-row')!;
        const button = li.querySelector('.sidebar-item-menu .icon-button')!;
        const [box, rowBox, buttonBox] = [li, row, button].map((each) =>
          each.getBoundingClientRect(),
        );
        return {
          tile: getComputedStyle(li).backgroundColor,
          row: `${getComputedStyle(row).backgroundColor} ${getComputedStyle(row).boxShadow}`,
          button: getComputedStyle(button).backgroundColor,
          inside:
            rowBox!.left >= box!.left - 0.5 &&
            buttonBox!.left >= rowBox!.right - 0.5 &&
            buttonBox!.right <= box!.right + 0.5,
        };
      });
    const resting = await tile();
    expect(resting.tile).not.toBe('rgba(0, 0, 0, 0)');
    expect(resting.row).toBe('rgba(0, 0, 0, 0) none');
    expect(resting.button).toBe('rgba(0, 0, 0, 0)');
    expect(resting.inside).toBe(true);
    await more.hover();
    expect(await tile()).toEqual(resting);
    await item.screenshot({ path: info.outputPath('session-tile.png') });

    // 设置 → 模型: the provider's models narrowed by words and to the ones ready to use; the
    // search, the filter, the connection and the buttons beside them are one strip.
    await page.getByRole('button', { name: '设置', exact: true }).click();
    const actions = page.locator('.model-section-actions');
    await expect(actions).toBeVisible();
    const titles = page.locator('.model-card .model-card-title strong');
    await expect(titles).toHaveText(['就绪模型', '草稿模型']);
    const count = page.locator('.model-section-count');
    await expect(count).toHaveText('2 个');
    await actions.getByRole('switch', { name: '只看已配置' }).click();
    await expect(titles).toHaveText(['就绪模型']);
    await expect(count).toHaveText('1/2 个');
    await actions.getByRole('switch', { name: '只看已配置' }).click();
    await actions.getByRole('searchbox', { name: '搜索模型' }).fill('DRAFT');
    await expect(titles).toHaveText(['草稿模型']);
    await actions.getByRole('switch', { name: '只看已配置' }).click();
    await expect(titles).toHaveCount(0);
    await expect(page.getByText('没有符合的模型')).toBeVisible();
    const heights = await actions
      .locator(':is(.model-section-search, select, .button, .icon-button)')
      .evaluateAll((controls) => controls.map((each) => each.getBoundingClientRect().height));
    expect(heights.length).toBeGreaterThanOrEqual(4);
    expect(new Set(heights)).toEqual(new Set([32]));
    // The filter's switch is drawn as every other switch, not as one of the row's buttons.
    const track = await actions.getByRole('switch', { name: '只看已配置' }).evaluate((control) => {
      const box = control.getBoundingClientRect();
      return `${box.width}x${box.height}`;
    });
    expect(track).toBe('34x18');
    await actions.screenshot({ path: info.outputPath('model-toolbar.png') });
    await actions.getByRole('searchbox', { name: '搜索模型' }).fill('');
    await actions.getByRole('switch', { name: '只看已配置' }).click();
    expect(await settled(page)).toEqual([]);
    // In a narrower window the name and its count keep one line, and the toolbar moves below them
    // whole instead of squeezing the name or breaking apart.
    const heading = page.locator('.provider-models-section > .section-heading-row');
    const resize = (width: number) =>
      app.evaluate(({ BrowserWindow }, width) => {
        const window = BrowserWindow.getAllWindows()[0]!;
        const [before, height] = window.getSize();
        window.setSize(width, height!);
        return before!;
      }, width);
    const layout = () =>
      heading.evaluate((row) => {
        const title = row.querySelector('.model-section-title')!.getBoundingClientRect();
        const middles = [...row.querySelectorAll('.model-section-actions > *')]
          .map((each) => each.getBoundingClientRect())
          .filter((box) => box.width > 0)
          .map((box) => Math.round(box.top + box.height / 2));
        return { titleHeight: Math.round(title.height), toolbarLines: new Set(middles).size };
      });
    const wide = await resize(1000);
    for (const width of [1000, 820]) {
      await resize(width);
      await expect.poll(async () => (await layout()).toolbarLines).toBe(1);
      expect((await layout()).titleHeight).toBeLessThan(28);
      await heading.screenshot({ path: info.outputPath(`model-heading-${width}.png`) });
    }
    await resize(wide);
    await heading.screenshot({ path: info.outputPath(`model-heading-${wide}.png`) });

    // The catalog, expanded: an icon is read once it comes near the view, and the ones further
    // down follow the scroll. Core fetches the icons from models.dev a few at a time after the
    // catalog loads, so the cards first wait for theirs to be named.
    const catalog = page.getByRole('region', { name: '未配置目录' });
    await catalog.getByRole('button', { name: /^查看更多/u }).click();
    const cards = await catalog.locator('.provider-card-shell').count();
    const named = catalog.locator('.provider-mark > :not(.provider-initials)');
    const icons = catalog.locator('.provider-mark > img');
    await expect.poll(() => named.count(), { timeout: 150_000 }).toBeGreaterThan(100);
    await page.waitForTimeout(1_500);
    const [withIcon, near] = [await named.count(), await icons.count()];
    console.log(
      `L18.11 catalog: ${cards} providers, ${withIcon} with an icon, ${near} read before scrolling`,
    );
    expect(near).toBeGreaterThan(0);
    expect(near).toBeLessThan(withIcon / 2);
    // The furthest card whose icon is named but not yet read comes into view and reads it. The
    // card is taken by its place, as it stops matching "not yet read" once it has read its icon.
    const shells = catalog.locator('.provider-card-shell');
    const farthest = await shells.evaluateAll((all) => {
      let last = -1;
      all.forEach((card, index) => {
        if (card.querySelector('.provider-mark > span:not(.provider-initials)')) last = index;
      });
      return last;
    });
    expect(farthest).toBeGreaterThan(0);
    const far = shells.nth(farthest);
    await far.scrollIntoViewIfNeeded();
    await expect(far.locator('.provider-mark > img')).toHaveCount(1);
    expect(await icons.count()).toBeGreaterThan(near);
    await catalog.screenshot({ path: info.outputPath('catalog-expanded.png') });

    // 权限与沙箱 reads its default after all of that.
    const nav = (name: string) =>
      page.locator('.settings-navigation').getByRole('button', { name, exact: true });
    await nav('权限与沙箱').click();
    await expect(page.locator('.permission-default-current')).toHaveText('当前默认');

    // 网络与浏览器: the browser switch, on by default, is drawn where it is from its first frame.
    await page.mouse.move(0, 0);
    await page.evaluate(() => {
      const frames: string[] = [];
      Reflect.set(window, 'auraSwitchFrames', frames);
      const started = performance.now();
      const sample = () => {
        const thumb = document.querySelector('[aria-label="启用内置浏览器"] .aura-switch-thumb');
        if (thumb !== null) {
          const style = getComputedStyle(thumb);
          frames.push(style.visibility === 'hidden' ? 'hidden' : style.transform);
        }
        if (performance.now() - started < 1_500) requestAnimationFrame(sample);
      };
      requestAnimationFrame(sample);
    });
    await nav('网络与浏览器').click();
    const browserSwitch = page.getByRole('switch', { name: '启用内置浏览器' });
    await expect(browserSwitch).toHaveAttribute('data-state', 'checked');
    await page.waitForTimeout(1_600);
    const frames = await page.evaluate(() => Reflect.get(window, 'auraSwitchFrames') as string[]);
    await info.attach('switch-frames', { body: JSON.stringify(frames), contentType: 'text/plain' });
    expect(frames.length).toBeGreaterThan(10);
    expect([...new Set(frames.filter((frame) => frame !== 'hidden'))]).toEqual([
      'matrix(1, 0, 0, 1, 16, 0)',
    ]);
    // A switch the user works still moves, unless Windows is set to show no animations.
    await browserSwitch.hover();
    const reduced = await page.evaluate(
      () => matchMedia('(prefers-reduced-motion: reduce)').matches,
    );
    expect(
      await browserSwitch
        .locator('.aura-switch-thumb')
        .evaluate((thumb) => getComputedStyle(thumb).transitionDuration),
    ).toBe(reduced ? '0s' : '0.14s');

    // Nothing was refused for want of room and the connection never dropped.
    const events = await mainEvents(root);
    const counted = failures(events);
    await info.attach('main-failures', {
      body: JSON.stringify(counted),
      contentType: 'application/json',
    });
    expect(events.filter(({ event }) => event.startsWith('core_transport'))).toEqual([]);
    expect(Object.keys(counted).filter((key) => key.startsWith('ipc.'))).toEqual([]);
  } finally {
    await app.close();
  }
});
