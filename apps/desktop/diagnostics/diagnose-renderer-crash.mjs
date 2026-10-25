// Renderer-crash reproduction: crash the renderer, confirm the rebuilt window
// reaches Ready, and measure the Core-backed commands the new page issues.
import { join } from 'node:path';

import { runDiagnostic, launch, writeJson } from './harness.mjs';

await runDiagnostic('renderer-crash', async ({ dir, log }) => {
  const application = await launch({
    dataRoot: join(dir, 'data'),
    environment: { AURA_IPC_TRACE_FILE: join(dir, 'core-trace.log') },
  });
  const first = await application.firstWindow();
  await first.getByText('Aura Core 已就绪').waitFor({ state: 'visible', timeout: 20000 });
  log('first window ready');

  const rebuiltPromise = application.waitForEvent('window', { timeout: 15000 });
  await application.evaluate(({ BrowserWindow }) => {
    BrowserWindow.getAllWindows()[0]?.webContents.forcefullyCrashRenderer();
  });
  log('renderer crashed');
  const rebuilt = await rebuiltPromise;
  log('rebuilt window received');

  let ready = false;
  try {
    await rebuilt.getByText('Aura Core 已就绪').waitFor({ state: 'visible', timeout: 15000 });
    ready = true;
    log('REBUILT WINDOW READY');
  } catch {
    log('REBUILT WINDOW NOT READY');
  }
  const probes = await rebuilt.evaluate(async () => {
    const api = globalThis.aura.v1;
    const attempt = async (name, run) => {
      const started = Date.now();
      try {
        const value = await run();
        return { name, ms: Date.now() - started, ok: value?.ok, code: value?.error?.code };
      } catch (error) {
        return { name, ms: Date.now() - started, threw: String(error).slice(0, 120) };
      }
    };
    return [
      await attempt('getCoreHealth', () => api.getCoreHealth()),
      await attempt('getModelSettings', () => api.getModelSettings()),
      await attempt('getWorkspaceSessions', () => api.getWorkspaceSessions()),
    ];
  });
  log('probes after rebuild:', JSON.stringify(probes));
  writeJson(dir, 'result.json', { ready, probes });
  if (!ready) {
    throw new Error('the rebuilt window never reported Aura Core ready');
  }
  return application;
});
