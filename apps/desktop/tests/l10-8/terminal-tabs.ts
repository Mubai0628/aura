import { expect, type Page } from '@playwright/test';

/**
 * A terminal tab opens the terminal chosen in 设置 → 环境 → 默认终端 at once;
 * the sidebar has no shell to pick. The model's terminal becomes a tab when the user asks to see
 * it. These are the steps a person takes.
 */
export async function openTerminalTab(page: Page) {
  // A narrow window hides the sidebar again when the conversation is shown anew (back from 设置).
  const tools = page.getByRole('button', { name: '打开工具', exact: true });
  const reopen = page.getByRole('button', { name: '展开右侧边栏', exact: true });
  await expect(tools.or(reopen)).toBeVisible();
  if (await reopen.isVisible()) await reopen.click();
  await tools.click();
  await page
    .getByRole('menu', { name: '选择工具' })
    .getByRole('menuitem', { name: /^终端/u })
    .click();
}

export async function chooseDefaultTerminal(page: Page, name: 'PowerShell 7' | 'CMD') {
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page
    .locator('.settings-navigation')
    .getByRole('button', { name: '环境', exact: true })
    .click();
  const choice = page
    .getByRole('radiogroup', { name: '默认终端' })
    .getByRole('radio', { name, exact: true });
  await expect(choice).toBeEnabled();
  // The choice shows as made once Core has saved it.
  await choice.click();
  await expect(choice).toBeChecked();
  await expect(choice).toBeEnabled();
  await page.getByRole('button', { name: '返回会话' }).click();
}

/** From 进程: the model terminal's row opens its tab. */
export async function viewModelTerminal(page: Page) {
  await page.getByRole('button', { name: '打开工具', exact: true }).click();
  await page.getByRole('menuitem', { name: /^进程/u }).click();
  await page
    .getByRole('region', { name: '会话后台进程' })
    .locator('li')
    .filter({ hasText: 'Agent 终端' })
    .getByRole('button', { name: '查看终端', exact: true })
    .click();
}
