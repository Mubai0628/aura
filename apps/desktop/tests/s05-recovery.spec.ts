import { _electron as electron, expect, test, type ElectronApplication } from '@playwright/test';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import type { AuraApiV1 } from '../shared/aura-api';

async function launch(root: string): Promise<ElectronApplication> {
  return electron.launch({
    args: [`--user-data-dir=${join(root, 'Electron')}`, resolve(import.meta.dirname, '..')],
    env: {
      ...process.env,
      APPDATA: join(root, 'Roaming'),
      LOCALAPPDATA: root,
      AURA_MEMORY_TEST_ROOT: join(root, 'global-memories'),
      AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
      AURA_E2E: '1',
      AURA_E2E_CORE_MODE: 'normal',
      AURA_P05_CLEAN_ACCEPTANCE: '1',
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
  });
}
async function partition(root: string): Promise<string> {
  const names = await readdir(join(root, 'Aura', 'namespaces'));
  expect(names).toHaveLength(1);
  return names[0]!;
}

// eslint-disable-next-line no-empty-pattern -- Fixtures are created explicitly.
test('S05 damaged save list requires explicit all-domain reset and preserves the original', async ({}, info) => {
  test.setTimeout(120_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s05-all-reset-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  let app: ElectronApplication | undefined;
  try {
    app = await launch(root);
    let page = await app.firstWindow();
    await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 30_000 });
    const namespace = await partition(root);
    await app.close();
    app = undefined;
    const transactions = join(root, 'Aura/namespaces', namespace, 'runtime/config-transactions');
    await mkdir(join(transactions, 'module-bundles'), { recursive: true });
    const original = 's05-damaged-save-list-original';
    await writeFile(join(transactions, 'module-bundles/fault.json'), original);
    app = await launch(root);
    page = await app.firstWindow();
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await expect(page.getByRole('heading', { name: '设置需要恢复' })).toBeVisible({
      timeout: 30_000,
    });
    await page.getByRole('button', { name: '查看恢复选项' }).click();
    const confirm = page.getByRole('button', { name: '确认重置全部配置', exact: true });
    await expect(confirm).toBeDisabled();
    expect(await readFile(join(transactions, 'module-bundles/fault.json'), 'utf8')).toBe(original);
    await page.getByRole('checkbox', { name: '我确认重置全部配置，现有非保密设置将丢失' }).check();
    await confirm.click();
    await expect(page.getByText('设置已恢复。', { exact: true })).toBeVisible({ timeout: 30_000 });
    expect(await readFile(join(transactions, 'recovery-originals/fault.json'), 'utf8')).toBe(
      original,
    );
    const notice = await page.evaluate(async () =>
      (window as unknown as { aura: { v1: AuraApiV1 } }).aura.v1.getConfigRecoveryNotice(),
    );
    expect(notice.ok).toBe(true);
    if (notice.ok) {
      expect(notice.value.scope_known).toBe(true);
      expect(notice.value.affected_domains).toEqual([]);
    }
    await info.attach('actual-all-reset-notice', {
      body: JSON.stringify(notice),
      contentType: 'application/json',
    });
    await page.screenshot({ path: info.outputPath('all-reset.png') });
  } finally {
    await app?.close();
  }
});

// eslint-disable-next-line no-empty-pattern -- Playwright requires destructuring its fixture argument.
test('S05 settings recovery uses the real authenticated Core and retains the fault original', async ({}, info) => {
  test.setTimeout(120_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s05-recovery-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  let app: ElectronApplication | undefined;
  try {
    app = await launch(root);
    let page = await app.firstWindow();
    await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 30_000 });
    const namespace = await partition(root);
    const configuration = join(root, 'Roaming', 'Aura', 'namespaces', namespace, 'tools.json');
    await app.close();
    app = undefined;
    // This is our own fixture input. Remove its valid history so the product must report a fault.
    await rm(join(root, 'Aura', 'namespaces', namespace, 'config-history', 'tools'), {
      recursive: true,
      force: true,
    });
    const damaged = '{"schema_version":1,"revision":7,"values":BROKEN-S05-ORIGINAL}';
    await writeFile(configuration, damaged);
    app = await launch(root);
    page = await app.firstWindow();
    await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 30_000 });
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await expect(page.getByRole('heading', { name: '设置需要恢复' })).toBeVisible();
    await expect(page.locator('.notification-viewport')).not.toContainText('core.model_authority');
    await expect(page.locator('.notification-viewport')).not.toContainText(
      'core.security_authority',
    );
    expect(await readFile(configuration, 'utf8')).toBe(damaged);
    await page.getByRole('button', { name: '查看恢复选项' }).click();
    await expect(page.getByRole('heading', { name: '确认恢复范围' })).toBeVisible();
    expect(await readFile(configuration, 'utf8')).toBe(damaged);
    await page.getByRole('radio', { name: '重置为默认值', exact: true }).check();
    const before = await page.evaluate(async () => {
      const api = (window as unknown as { aura: { v1: AuraApiV1 } }).aura.v1;
      return api.planConfigRecovery({ domains: ['tools'] });
    });
    expect(before.ok).toBe(true);
    if (!before.ok) throw new Error('real Core preview did not arrive');
    expect(before.value.preview_id.length).toBeGreaterThan(0);
    await page.getByRole('button', { name: '确认重置为默认值', exact: true }).click();
    await expect(page.getByText('设置已恢复。', { exact: true })).toBeVisible();
    const restored = JSON.parse(await readFile(configuration, 'utf8')) as {
      revision: number;
      values: unknown;
    };
    expect(restored.revision).toBeGreaterThan(0);
    const after = await page.evaluate(async () => {
      const api = (window as unknown as { aura: { v1: AuraApiV1 } }).aura.v1;
      const stale = await api.applyConfigRecovery({
        domain: 'tools',
        domains: [],
        action: 'reset_to_default',
        preview_id: '',
      });
      const notice = await api.getConfigRecoveryNotice();
      return { stale, notice };
    });
    expect(after.stale.ok).toBe(false);
    expect(after.notice.ok).toBe(true);
    if (after.notice.ok) {
      expect(after.notice.value.affected_domains).not.toContain('tools');
      expect(after.notice.value.recovery_in_progress).toBe(false);
    }
    // Original-byte preservation is measured in this run's actual quarantine products.
    async function findBytes(directory: string): Promise<boolean> {
      let entries;
      try {
        entries = await readdir(directory, { withFileTypes: true });
      } catch {
        return false;
      }
      for (const entry of entries) {
        const path = join(directory, entry.name);
        if (entry.isDirectory()) {
          if (await findBytes(path)) return true;
        } else if ((await readFile(path)).toString('utf8') === damaged) return true;
      }
      return false;
    }
    expect(
      await findBytes(join(root, 'Aura', 'namespaces', namespace, 'config-history', 'tools')),
    ).toBe(true);
    expect(errors).toEqual([]);
    // Failures stay until closed, but the ones that described the broken
    // settings are withdrawn once the settings work again; the restored view shows a usable consumer.
    await expect(page.getByRole('heading', { name: '模型配置', exact: true })).toBeVisible();
    await expect(page.locator('.notification-viewport')).not.toContainText('模型配置不可用');
    await expect(page.locator('.notification-viewport')).not.toContainText('权限与沙箱操作未完成');
    await page.screenshot({ path: info.outputPath('s05-recovery.png') });
    await info.attach('real-core-recovery-facts', {
      body: JSON.stringify({ before, after, restored }),
      contentType: 'application/json',
    });
  } finally {
    await app?.close(); /* Preserve the isolated first-failure inputs and products. */
  }
});
