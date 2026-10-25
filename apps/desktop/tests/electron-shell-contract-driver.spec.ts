import { expect, test, type ElectronApplication } from '@playwright/test';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  RendererResult,
  launchAuraWithDataRoot,
  launchProductionAura,
  launchAura,
  closeAura,
} from './electron-shell/harness.js';

test('P08 Contract Driver is compile-time isolated and crosses the real P07 Built-in path', async () => {
  test.setTimeout(180_000);
  const application = await launchAura();
  try {
    const window = await application.firstWindow();
    await expect(window.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 15_000 });
    const invoke = (probe: 'invalid_identity' | 'invalid_token' | 'valid') =>
      window.evaluate(async (requestedProbe) => {
        const rendererGlobal = globalThis as typeof globalThis & {
          auraP08ContractDriver: { run(probe: typeof requestedProbe): Promise<unknown> };
        };
        return rendererGlobal.auraP08ContractDriver.run(requestedProbe);
      }, probe) as Promise<RendererResult<Record<string, unknown>>>;

    const invalidToken = await invoke('invalid_token');
    expect(invalidToken.ok).toBe(false);
    if (!invalidToken.ok)
      expect(invalidToken.error.code).toBe('p08.contract_driver.identity_rejected');

    const invalidIdentity = await invoke('invalid_identity');
    expect(invalidIdentity.ok).toBe(false);
    if (!invalidIdentity.ok)
      expect(invalidIdentity.error.code).toBe('p08.contract_driver.identity_rejected');

    const result = await invoke('valid');
    expect(result).toEqual({
      ok: true,
      value: {
        electron_e2e_contract_driver: 'passed',
        fixture_dispatch_count: 7,
        isolated_runtime_profile: 'p07-isolated-local-fixtures-v1',
        p08_production_agent_loop: 'deferred_to_p08',
        production_p05_to_p08_trigger_count: 0,
      },
    });
  } finally {
    await closeAura(application);
  }
});

test('P08 Contract Driver rejects the E2E build outside the isolated runtime profile', async () => {
  const localAppData = await mkdtemp(join(tmpdir(), 'aura-electron-driver-profile-'));
  let application: ElectronApplication | undefined;
  try {
    application = await launchAuraWithDataRoot(localAppData, 'normal', {
      environment: { AURA_P08_DRIVER_WITHOUT_PROFILE: '1' },
    });
    const window = await application.firstWindow();
    await expect(window.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 15_000 });
    const result = (await window.evaluate(() => {
      const rendererGlobal = globalThis as typeof globalThis & {
        auraP08ContractDriver: { run(): Promise<unknown> };
      };
      return rendererGlobal.auraP08ContractDriver.run();
    })) as RendererResult<unknown>;
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error.code).toBe('p08.contract_driver.profile_required');
  } finally {
    if (application === undefined) {
      await rm(localAppData, { force: true, recursive: true });
    } else {
      await closeAura(application);
    }
  }
});

test('AURA_E2E alone cannot expose the P08 Contract Driver in the production entry', async () => {
  const application = await launchProductionAura();
  try {
    const window = await application.firstWindow();
    await expect(window).toHaveTitle('Aura');
    expect(
      await window.evaluate(() => typeof Reflect.get(globalThis, 'auraP08ContractDriver')),
    ).toBe('undefined');
  } finally {
    await closeAura(application);
  }
});
