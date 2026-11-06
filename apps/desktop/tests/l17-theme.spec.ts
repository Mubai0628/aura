import { expect, test, type Page } from '@playwright/test';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { launch } from './s06/harness';
import { expectSecureSurface } from './electron-shell/renderer-api';
import { addAndTrust, pluginDialog, pluginSettings } from './l17/setup';

const MANIFEST = `schema_version = 2
name = "night-theme"
version = "0.1.0"
description = "夜间主题"
`;
const theme = (canvas: string) => `title = "夜间"
scheme = "dark"

[colors]
canvas = "${canvas}"
ink = "#e6e6e6"

[radius]
md = 6
`;

/** What the window shows now: its canvas colour and whether it is drawn dark. */
const shown = (page: Page) =>
  page.evaluate(() => {
    const style = getComputedStyle(document.documentElement);
    return { canvas: style.getPropertyValue('--canvas').trim(), scheme: style.colorScheme };
  });

// eslint-disable-next-line no-empty-pattern -- real Electron/Core and the plugin page, a TEMP plugin folder and isolated settings; no model.
test('L17 a plugin theme in development is chosen in 设置 → 环境, follows its file and gives way when untrusted', async ({}, info) => {
  test.setTimeout(240_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l17-theme-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const folder = join(root, 'night-theme');
  await mkdir(folder);
  await writeFile(join(folder, 'aura-plugin.toml'), MANIFEST);
  await writeFile(join(folder, 'theme.toml'), theme('#1e1f1c'));
  const { app, page } = await launch(root);
  try {
    await expectSecureSurface(page);
    const own = await shown(page);
    expect(own.canvas).not.toBe('#1e1f1c');
    const row = await addAndTrust(app, page, folder, 'night-theme');

    // Chosen as the user chooses it: the window takes its colours and is drawn dark.
    await page
      .getByRole('navigation', { name: '设置分类' })
      .getByRole('button', { name: '环境', exact: true })
      .click();
    const themes = page
      .getByRole('region', { name: '外观' })
      .getByRole('radiogroup', { name: '主题' });
    // The choice shows once Core has kept it.
    const night = themes.getByRole('radio', { name: '夜间' });
    await night.click();
    await expect(night).toBeChecked();
    await expect.poll(() => shown(page)).toEqual({ canvas: '#1e1f1c', scheme: 'dark' });

    // A plugin in development is read where it is written: the changed file shows once the
    // window comes back to the front, with no new trust.
    await writeFile(join(folder, 'theme.toml'), theme('#202124'));
    await page.evaluate(() => window.dispatchEvent(new Event('focus')));
    await expect.poll(() => shown(page)).toEqual({ canvas: '#202124', scheme: 'dark' });

    // Untrusted, it can give nothing: Aura's own theme shows, and the choice says why.
    await pluginSettings(page);
    const dialog = await pluginDialog(page, row, 'night-theme');
    await dialog.getByRole('button', { name: '撤销信任' }).click();
    await expect(row).toContainText('未信任');
    await dialog.getByRole('button', { name: '关闭' }).click();
    await page.evaluate(() => window.dispatchEvent(new Event('focus')));
    await expect.poll(() => shown(page)).toEqual(own);
    await page
      .getByRole('navigation', { name: '设置分类' })
      .getByRole('button', { name: '环境', exact: true })
      .click();
    await expect(themes.getByRole('radio', { name: 'night-theme（现在不能用）' })).toBeChecked();
  } finally {
    await app.close();
  }
});
