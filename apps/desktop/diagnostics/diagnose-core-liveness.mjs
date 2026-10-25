// Request-level liveness of the Core channel: measures how long Core-backed
// commands take, including the readiness probe that used to monopolise the
// connection's read loop, and verifies a request issued *while* readiness is in
// flight is still answered.
import { join } from 'node:path';

import { runDiagnostic, launch, writeJson } from './harness.mjs';

await runDiagnostic('core-liveness', async ({ dir, log }) => {
  const application = await launch({
    dataRoot: join(dir, 'data'),
    environment: { AURA_IPC_TRACE_FILE: join(dir, 'core-trace.log') },
  });
  const window = await application.firstWindow();
  await window.getByText('Aura Core 已就绪').waitFor({ state: 'visible', timeout: 20000 });
  log('core ready');

  // Start the slow probe without awaiting it, then issue a fast command on the
  // same channel. Serialisation in the reader would make the second command
  // wait for the first to finish.
  const overlap = await window.evaluate(async () => {
    const api = globalThis.aura.v1;
    const readinessStarted = Date.now();
    const readiness = api.getRuntimeReadiness();
    const settingsStarted = Date.now();
    const settings = await api.getModelSettings();
    const settingsMs = Date.now() - settingsStarted;
    const readinessValue = await readiness;
    return {
      readinessMs: Date.now() - readinessStarted,
      readinessOk: readinessValue?.ok,
      settingsMs,
      settingsOk: settings?.ok,
    };
  });
  log('overlap probe:', JSON.stringify(overlap));
  writeJson(dir, 'result.json', overlap);
  if (overlap.settingsOk !== true) {
    throw new Error('a fast command issued during readiness was not answered');
  }
  if (overlap.settingsMs > 2000) {
    throw new Error(
      `the fast command waited ${overlap.settingsMs}ms behind readiness; the channel is serialised again`,
    );
  }
  return application;
});
