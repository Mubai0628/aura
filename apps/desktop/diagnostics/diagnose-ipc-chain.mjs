// Full-chain IPC tracer: does a request that the Core answered ever reach the
// renderer, and if not, at which boundary is it lost?
import { join } from 'node:path';

import { runDiagnostic, launch, writeJson } from './harness.mjs';

await runDiagnostic('ipc-chain', async ({ dir, log }) => {
  const traceFile = join(dir, 'core-trace.log');
  const application = await launch({
    dataRoot: join(dir, 'data'),
    environment: { AURA_IPC_TRACE_FILE: traceFile },
  });
  const window = await application.firstWindow();
  await window.getByText('Aura Core 已就绪').waitFor({ state: 'visible', timeout: 20000 });
  log('core ready');

  const probe = async (label) => {
    const before = Date.now();
    const outcome = await window.evaluate(async () => {
      const started = Date.now();
      try {
        const value = await globalThis.aura.v1.getModelSettings();
        return { ms: Date.now() - started, ok: value?.ok, code: value?.error?.code };
      } catch (error) {
        return { ms: Date.now() - started, threw: String(error).slice(0, 120) };
      }
    });
    log(`probe ${label}`, JSON.stringify(outcome), `wall=${Date.now() - before}`);
    return outcome;
  };

  await probe('before-interaction');

  // runtime.readiness is the request that used to hang the channel: measure it
  // on its own before touching any scoped projection.
  const readiness = await window.evaluate(async () => {
    const started = Date.now();
    const value = await globalThis.aura.v1.getRuntimeReadiness();
    return { ms: Date.now() - started, ok: value?.ok, code: value?.error?.code };
  });
  log('readiness before interaction', JSON.stringify(readiness));

  // The spec's real interaction order: open and close the quick model menu,
  // which mounts and unmounts a scoped projection subscription.
  await window.getByRole('button', { name: /模型未选择/u }).click();
  await window
    .getByRole('menu', { name: '快速选择模型' })
    .waitFor({ state: 'visible', timeout: 5000 });
  await window.getByRole('heading', { name: '实现模型配置与 Transport' }).click();
  await window
    .getByRole('menu', { name: '快速选择模型' })
    .waitFor({ state: 'hidden', timeout: 5000 });
  await window.getByRole('button', { name: /^文件/u }).click();
  await window.getByRole('searchbox', { name: '筛选文件' }).waitFor({ timeout: 5000 });
  await window.getByRole('button', { name: '设置' }).click();
  await window.getByRole('heading', { level: 1, name: '设置' }).waitFor({ timeout: 5000 });
  log('settings opened');

  try {
    await window
      .getByRole('heading', { level: 2, name: '模型配置' })
      .waitFor({ state: 'visible', timeout: 12000 });
    log('MODEL HEADING VISIBLE');
  } catch {
    log('MODEL HEADING MISSING');
  }
  await probe('after-interaction');

  const mainLogRoot = join(dir, 'data', 'Aura', 'namespaces');
  writeJson(dir, 'status.json', {
    traceFile,
    mainLogRoot,
    headingVisible: await window
      .getByRole('heading', { level: 2, name: '模型配置' })
      .isVisible()
      .catch(() => false),
  });
  return application;
});
