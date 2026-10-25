/* global document, window */
// Real-window check for the archived-Workspace rule: the frozen design keeps the
// main list for active work ("主列表只提供归档", archived ones live in 设置 → 归档),
// so an archived Workspace must not keep a sidebar row — while its registry row,
// Sessions and files stay intact and `workspace.restore` can bring it back.
//
// It launches the built desktop app against the operator's real registry (no data
// root override, nothing written), reads the Workspace registry projection, and
// compares it with what the sidebar actually renders. Evidence goes to
// target/r10-sidebar-archived/.
import { _electron as electron } from '@playwright/test';
import { mkdir, rm, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

const desktop = resolve(import.meta.dirname, '..');
const output = resolve(desktop, '../../target/r10-sidebar-archived');
const electronProfile = join(output, 'Electron');
await rm(electronProfile, { recursive: true, force: true });
await mkdir(electronProfile, { recursive: true });

const report = { steps: [] };
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

  const registry = await page.evaluate(async () => {
    const result = await window.aura.v1.getWorkspaceSessions();
    return result.ok
      ? result.value.payload.workspaces.map((workspace) => ({
          id: workspace.workspace_id,
          name: workspace.display_name,
          state: workspace.state,
        }))
      : { error: result.error.code };
  });
  record('registry workspaces', registry);

  // The sidebar renders after the projection lands; give it a real render turn.
  await page.waitForTimeout(2000);
  const sidebar = await page.evaluate(() =>
    [...document.querySelectorAll('.workspace-row')].map((row) => ({
      name: row.querySelector('.workspace-select')?.textContent ?? '',
      root: row.querySelector('.workspace-select')?.getAttribute('title') ?? '',
    })),
  );
  record('sidebar workspaces', sidebar);

  const archived = Array.isArray(registry) ? registry.filter((w) => w.state === 'archived') : [];
  const live = Array.isArray(registry) ? registry.filter((w) => w.state !== 'archived') : [];
  report.checks = {
    // The rule under test: archived Workspaces own no sidebar row. Sessions nested
    // under them leave with the row, so the row set is what matters here.
    no_archived_workspace_row: archived.every((w) => !sidebar.some((row) => row.name === w.name)),
    every_live_workspace_listed: live.every((w) => sidebar.some((row) => row.name === w.name)),
    sidebar_rows_match_live_workspaces: sidebar.length === live.length,
  };
  report.archived = archived;
  report.live = live;
  report.sidebar = sidebar;
  report.passed = Object.values(report.checks).every((value) => value === true);
  record('checks', report.checks);
  record('passed', report.passed);
  await page.screenshot({ path: join(output, 'sidebar.png') });
  await writeFile(join(output, 'sidebar.json'), JSON.stringify(report, null, 2));
  process.exitCode = report.passed ? 0 : 1;
} catch (error) {
  report.error = error.message.slice(0, 1000);
  await writeFile(join(output, 'failure.json'), JSON.stringify(report, null, 2));
  console.error(`FAILED: ${report.error}`);
  process.exitCode = 1;
} finally {
  await app?.close();
}
