import { ipcMain, type IpcMainInvokeEvent } from 'electron';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';

const MAIN_BUILD_EXTENSION_SYMBOL = Symbol.for('aura.main-build-extension.v1');
const MAIN_ROUTE = 'aura:e2e:p08-contract-driver';
const CORE_METHOD = 'internal.e2e.p08_contract_driver.run_vertical_suite';
const token = randomBytes(32).toString('hex');
const isolatedProfileEnabled = process.env.AURA_P08_DRIVER_WITHOUT_PROFILE !== '1';
const isolatedWorkspaceFixtureRoot = process.env.AURA_P07_E2E_SESSION_FIXTURE_ROOT;
const additionalCoreArguments = isolatedProfileEnabled
  ? [
      '--p08-contract-driver-profile=p07-isolated-local-fixtures-v1',
      `--p08-contract-driver-token=${token}`,
      ...(isolatedWorkspaceFixtureRoot === undefined
        ? []
        : [`--p08-contract-driver-workspace-fixture-root=${isolatedWorkspaceFixtureRoot}`]),
    ]
  : [];

type Probe = 'invalid_identity' | 'invalid_token' | 'valid';

interface ExtensionContext {
  assertTrustedSender(event: IpcMainInvokeEvent): void;
  invokeCore(
    method: string,
    params: (identity: { coreRuntimeInstanceId: string; mainRuntimeInstanceId: string }) => unknown,
  ): Promise<unknown>;
}

Reflect.set(globalThis, MAIN_BUILD_EXTENSION_SYMBOL, {
  additionalCoreArguments,
  preloadPath: resolve(import.meta.dirname, '../preload/index.cjs'),
  registerIpc(context: ExtensionContext): void {
    ipcMain.handle(MAIN_ROUTE, async (event, input: unknown) => {
      try {
        context.assertTrustedSender(event);
        if (
          typeof input !== 'object' ||
          input === null ||
          Array.isArray(input) ||
          !('probe' in input) ||
          !['invalid_identity', 'invalid_token', 'valid'].includes(String(input.probe))
        ) {
          return { error: { code: 'e2e.driver.request_invalid' }, ok: false };
        }
        const probe = input.probe as Probe;
        const value = await context.invokeCore(CORE_METHOD, (identity) => ({
          claimed_core_runtime_instance_id:
            probe === 'invalid_identity'
              ? 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV'
              : identity.coreRuntimeInstanceId,
          claimed_main_runtime_instance_id: identity.mainRuntimeInstanceId,
          token: probe === 'invalid_token' ? '0'.repeat(64) : token,
        }));
        return { ok: true, value };
      } catch (error) {
        const envelope =
          error instanceof Error && 'envelope' in error
            ? (error as { envelope: unknown }).envelope
            : { code: 'e2e.driver.main_failed', message: String(error) };
        return { error: envelope, ok: false };
      }
    });
  },
});

await import(new URL('../../electron-main/main.js', import.meta.url).href);
