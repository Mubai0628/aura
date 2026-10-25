import { expect, test } from '@playwright/test';
import { closeAura, launchAura } from './electron-shell/harness.js';

// At the window's smallest height the settings sections do not all fit; their list scrolls on
// its own, so the wheel brings the last section into sight and it opens.
test('in a short window the settings section list scrolls to its last section', async () => {
  const application = await launchAura();
  try {
    const window = await application.firstWindow();
    await expect(window.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 15_000 });
    await application.evaluate(({ BrowserWindow }) => {
      BrowserWindow.getAllWindows()[0]?.setSize(1_200, 540);
    });
    await expect.poll(() => window.evaluate(() => innerHeight)).toBeLessThan(540);
    await window.getByRole('button', { name: '设置', exact: true }).click();
    const sections = window.getByRole('navigation', { name: '设置分类' });
    const last = sections.getByRole('button', { name: '关于与更新' });
    const inSight = () =>
      last.evaluate((element) => element.getBoundingClientRect().bottom <= innerHeight);
    expect(await inSight(), 'the last section starts below the window').toBe(false);
    const box = await sections.boundingBox();
    expect(box).not.toBeNull();
    await window.mouse.move(box!.x + box!.width / 2, box!.y + 120);
    await window.mouse.wheel(0, 2_000);
    await expect.poll(inSight).toBe(true);
    await last.click();
    await expect(window.getByRole('heading', { level: 2, name: '关于与更新' })).toBeVisible();
  } finally {
    await closeAura(application);
  }
});
