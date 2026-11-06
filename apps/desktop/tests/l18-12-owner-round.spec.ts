import { expect, test, type Page } from '@playwright/test';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armOwnerRound } from './l18-11/arm';
import { addSecondProvider, thumbTravel } from './l18-12/arm';
import { pixelAudit, type PixelFinding } from './pixel/audit';
import { launch } from './s06/harness';

// In the real window with the real Core; no model is asked anything.

/** The pixel audit, given the page a few moments to settle. */
async function settled(page: Page) {
  let findings: PixelFinding[] = await pixelAudit(page);
  for (let tries = 0; findings.length > 0 && tries < 10; tries += 1) {
    await page.waitForTimeout(500);
    findings = await pixelAudit(page);
  }
  return findings;
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core in TEMP; no model is asked anything.
test('L18.12 the sidebar foot, the model page, each provider’s filter, its switch and where turns operate programs', async ({}, info) => {
  test.setTimeout(240_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l18-12-owner-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace, { recursive: true });
  const { app, page } = await launch(root);
  try {
    const armed = await armOwnerRound(page, workspace);
    expect(armed).not.toHaveProperty('error');
    expect(await addSecondProvider(page)).toEqual({ ok: true });
    await expect(page.locator('.app-shell[data-core-state="ready"]')).toBeVisible({
      timeout: 60_000,
    });

    // The sidebar's foot holds 设置 and nothing else.
    await page.locator('.session-row').first().click();
    const foot = page.locator('.sidebar-footer');
    await expect(foot.locator(':scope > *')).toHaveCount(1);
    await expect(foot).toHaveText('设置');
    for (const gone of ['Aura Core 已就绪', '本地运行 · 无需登录'])
      await expect(page.getByText(gone)).toHaveCount(0);

    // No 侧边对话: neither in the tool menu nor behind Ctrl+Alt+S.
    await page.getByRole('button', { name: '打开工具' }).click();
    const tools = page.getByRole('menu', { name: '选择工具' }).getByRole('menuitem');
    await expect(tools.first()).toBeVisible();
    expect(await tools.allTextContents()).not.toContainEqual(expect.stringContaining('侧边对话'));
    await page.keyboard.press('Escape');
    await page.keyboard.press('Control+Alt+S');
    await expect(page.getByText(/侧边对话/u)).toHaveCount(0);

    // 设置 → 模型: refreshing the catalog is the title row's own button, at its end; the catalog has
    // no label or status line of its own.
    await page.getByRole('button', { name: '设置', exact: true }).click();
    const titleRow = page
      .locator('.settings-content-heading')
      .filter({ has: page.getByRole('heading', { name: '模型配置' }) });
    const refresh = titleRow.getByRole('button', { name: '刷新 models.dev 目录' });
    await expect(refresh).toBeVisible();
    const placed = await titleRow.evaluate((row) => {
      const title = row.querySelector('h2')!.getBoundingClientRect();
      const button = row.querySelector('button')!.getBoundingClientRect();
      const box = row.getBoundingClientRect();
      const padding = parseFloat(getComputedStyle(row).paddingRight);
      return {
        atEnd: Math.abs(box.right - padding - button.right) <= 1,
        level: Math.abs(title.top + title.height / 2 - (button.top + button.height / 2)) <= 4,
      };
    });
    expect(placed).toEqual({ atEnd: true, level: true });
    await expect(page.getByRole('button', { name: '刷新 models.dev 目录' })).toHaveCount(1);
    await expect(page.getByRole('heading', { name: 'Provider', exact: true })).toHaveCount(0);
    await expect(page.getByText(/models\.dev 目录 ·/u)).toHaveCount(0);
    await titleRow.screenshot({ path: info.outputPath('model-title-row.png') });

    // 只看已配置 is each provider's own: on for the first provider, still off for the second, and
    // on again for the first when the user comes back to it.
    const titles = page.locator('.model-card .model-card-title strong');
    const onlyReady = page.locator('.model-section-actions').getByRole('switch', {
      name: '只看已配置',
    });
    await expect(titles).toHaveText(['就绪模型', '草稿模型']);
    // The thumb glides there in many frames rather than jumping in two.
    await onlyReady.hover();
    const reduced = await page.evaluate(
      () => matchMedia('(prefers-reduced-motion: reduce)').matches,
    );
    const travel = await thumbTravel(page, '.model-section-actions .aura-switch-thumb', () =>
      onlyReady.click(),
    );
    await info.attach('switch-travel', { body: JSON.stringify(travel), contentType: 'text/plain' });
    const between = new Set(travel.filter((x) => x > 0.5 && x < 15.5).map((x) => x.toFixed(1)));
    if (reduced) expect(between.size).toBe(0);
    else expect(between.size).toBeGreaterThanOrEqual(3);
    expect(travel.at(-1)).toBe(16);
    await expect(titles).toHaveText(['就绪模型']);
    await page.getByRole('button', { name: /^L18\.12 乙.*个连接/u }).click();
    await expect(titles).toHaveText(['乙就绪', '乙草稿']);
    await expect(onlyReady).toHaveAttribute('aria-checked', 'false');
    await onlyReady.click();
    await expect(titles).toHaveText(['乙就绪']);
    await onlyReady.click();
    await expect(titles).toHaveText(['乙就绪', '乙草稿']);
    await page.getByRole('button', { name: /^L18\.11 服务.*个连接/u }).click();
    await expect(onlyReady).toHaveAttribute('aria-checked', 'true');
    await expect(titles).toHaveText(['就绪模型']);
    await page.mouse.move(0, 0);
    expect(await settled(page)).toEqual([]);

    // 设置 → 电脑操控: where new turns operate programs, and the keys that take over in either.
    await page
      .locator('.settings-navigation')
      .getByRole('button', { name: '电脑操控', exact: true })
      .click();
    await page.getByRole('switch', { name: '电脑操控' }).click();
    const mode = page.getByRole('combobox', { name: '操控方式' });
    await expect(mode).toHaveValue('foreground');
    expect(await mode.locator('option').allTextContents()).toEqual(['前台', '小窗']);
    await mode.selectOption({ label: '小窗' });
    await expect
      .poll(() =>
        page.evaluate(async () => {
          const read = await window.aura.v1.computer?.settings();
          return read?.ok ? [read.value.settings.enabled, read.value.settings.mode] : read;
        }),
      )
      .toEqual([true, 'child_session']);
    await expect(page.getByRole('textbox', { name: '停止快捷键' })).toHaveValue('Ctrl+Alt+Q');
    await expect(page.getByRole('textbox', { name: '小窗停止快捷键', exact: true })).toHaveCount(0);
    await page.locator('.computer-settings').screenshot({ path: info.outputPath('computer.png') });
    expect(await settled(page)).toEqual([]);

    // The composer offers no desktop of its own while computer control is on.
    await page.getByRole('button', { name: '返回会话' }).click();
    await expect(page.getByRole('textbox', { name: '输入消息' })).toBeVisible();
    await page.waitForTimeout(5_500);
    await expect(page.getByRole('button', { name: /^电脑：/u })).toHaveCount(0);
    expect(await settled(page)).toEqual([]);
  } finally {
    await app.close();
  }
});
