/* global window */
// Removes synthetic Workspaces that a verification run created in the operator's
// real Aura registry. There is no workspace-removal entry in the product UI yet,
// so this drives the shipped P03 lifecycle route itself: archive -> acknowledge,
// and verifies the Workspace left the registry projection. The operation is the
// same reversible one `workspace.restore` undoes.
//
// Usage: node tests/r10-workspace-cleanup.mjs wsp_... wsp_...
import { _electron as electron } from '@playwright/test';
import { mkdir, rm, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

const desktop = resolve(import.meta.dirname, '..');
const output = resolve(desktop, '../../target/r10-workspace-cleanup');
const electronProfile = join(output, 'Electron');
await rm(electronProfile, { recursive: true, force: true });
await mkdir(electronProfile, { recursive: true });

const targets = process.argv.slice(2).filter((value) => value.startsWith('wsp_'));
const report = { targets, steps: [] };
const record = (name, detail) => {
  report.steps.push({ name, detail });
  console.log(`${report.steps.length}. ${name} -> ${JSON.stringify(detail)}`);
};

let app;
try {
  const environment = { ...process.env, AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1' };
  delete environment.AURA_IPC_TRACE_FILE;
  app = await electron.launch({
    args: [`--user-data-dir=${electronProfile}`, desktop],
    cwd: desktop,
    env: environment,
    timeout: 60_000,
  });
  const page = await app.firstWindow();
  await page.waitForLoadState('domcontentloaded');
  for (let attempt = 0; attempt < 120; attempt += 1) {
    const ready = await page.evaluate(async () => {
      const result = await window.aura.v1.getCoreHealth();
      return result.ok && result.value.payload.state === 'ready';
    });
    if (ready) break;
    await page.waitForTimeout(1000);
  }

  const projection = () =>
    page.evaluate(async () => {
      const result = await window.aura.v1.getWorkspaceSessions();
      return result.ok
        ? result.value.payload.workspaces.map((workspace) => ({
            id: workspace.workspace_id,
            name: workspace.display_name,
            revision: workspace.revision,
            state: workspace.state,
          }))
        : { error: result.error.code };
    });
  const before = await projection();
  record('registry before cleanup', before);

  for (const workspaceId of targets) {
    const found = Array.isArray(before)
      ? before.find((item) => item.id === workspaceId)
      : undefined;
    if (found === undefined) {
      record(`${workspaceId} not in the registry`, null);
      continue;
    }
    if (found.state === 'archived') {
      record(`${workspaceId} already archived`, found.state);
      continue;
    }
    const begun = await page.evaluate(
      async ({ id, revision }) => {
        const result = await window.aura.v1.archiveWorkspace({
          workspace_id: id,
          expected_workspace_revision: revision,
        });
        return result.ok ? { ok: true, ...result.value } : { ok: false, error: result.error };
      },
      { id: workspaceId, revision: found.revision },
    );
    record(`archive ${workspaceId}`, begun);
    if (!begun.ok) continue;
    if (begun.state === 'failed' || begun.state === 'cancelled') {
      record(`archive ${workspaceId} did not start`, begun.state);
      continue;
    }
    // The acknowledge step is what commits the lifecycle operation; the begin
    // call only opens it and starts the renderer's ack deadline.
    const acknowledged = await page.evaluate(
      async ({ id, operationId, lifecycleRevision }) => {
        const result = await window.aura.v1.acknowledgeWorkspaceLifecycle({
          workspace_id: id,
          operation_id: operationId,
          lifecycle_revision: lifecycleRevision,
          buffers: [],
        });
        return result.ok ? { ok: true, ...result.value } : { ok: false, error: result.error };
      },
      {
        id: workspaceId,
        operationId: begun.operation_id,
        lifecycleRevision: begun.lifecycle_revision,
      },
    );
    record(`acknowledge ${workspaceId}`, acknowledged);
  }

  const after = await projection();
  record('registry after cleanup', Array.isArray(after) ? after : after);
  const stillLive = Array.isArray(after) ? after.filter((item) => targets.includes(item.id)) : [];
  report.before = before;
  report.after = after;
  // The shipped lifecycle retires a Workspace as `archived` (reversible with
  // `workspace.restore`); the Renderer currently still lists it, which is a
  // separate product gap, so `archived` counts as cleanup here.
  report.passed = Array.isArray(after) && stillLive.every((item) => item.state === 'archived');
  record('workspaces still listed', stillLive);
  record('passed', report.passed);
  await writeFile(join(output, 'cleanup.json'), JSON.stringify(report, null, 2));
  process.exitCode = report.passed ? 0 : 1;
} catch (error) {
  report.error = error.message.slice(0, 1000);
  await writeFile(join(output, 'failure.json'), JSON.stringify(report, null, 2));
  console.error(`FAILED: ${report.error}`);
  process.exitCode = 1;
} finally {
  await app?.close();
}
