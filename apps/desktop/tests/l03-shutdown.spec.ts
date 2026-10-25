import { _electron as electron, expect, test } from '@playwright/test';
import { execFile } from 'node:child_process';
import { mkdtemp, mkdir, readdir, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';
import { openTerminalTab } from './l10-8/terminal-tabs';

async function alive(pid: number): Promise<boolean> {
  const { stdout } = await promisify(execFile)('tasklist.exe', [
    '/FI',
    `PID eq ${pid}`,
    '/FO',
    'CSV',
    '/NH',
  ]);
  return stdout.includes(`"${pid}"`);
}

async function corePid(root: string): Promise<number> {
  const namespaces = join(root, 'Local', 'Aura', 'namespaces');
  const deadline = Date.now() + 5_000;
  do {
    for (const namespace of await readdir(namespaces).catch(() => [])) {
      const path = join(namespaces, namespace, 'logs', 'main.jsonl');
      const lines = await readFile(path, 'utf8').catch(() => '');
      // The asynchronous logger may still be appending the last record.
      const records = lines
        .split('\n')
        .slice(0, -1)
        .filter(Boolean)
        .map((line) => JSON.parse(line) as { event: string; fields: { pid?: number } });
      const record = records.findLast((entry) => entry.event === 'core_image');
      if (record?.fields.pid !== undefined) return record.fields.pid;
    }
    await new Promise((done) => setTimeout(done, 50));
  } while (Date.now() < deadline);
  throw new Error('temporary Main log did not bind the spawned Core PID');
}

for (const mode of ['escape', 'stream'] as const) {
  for (let repetition = 1; repetition <= 5; repetition += 1) {
    test(`L03 B0 shutdown ${mode} ${repetition}: Electron and Core both exit`, async ({
      browserName,
    }, info) => {
      void browserName;
      test.setTimeout(120_000);
      const root = await mkdtemp(join(tmpdir(), 'aura-l03-shutdown-'));
      const work = join(root, 'workspace');
      await mkdir(work);
      const app = await electron.launch({
        args: [`--user-data-dir=${join(root, 'Electron')}`, resolve(import.meta.dirname, '..')],
        env: {
          ...process.env,
          APPDATA: join(root, 'Roaming'),
          LOCALAPPDATA: join(root, 'Local'),
          AURA_MEMORY_TEST_ROOT: join(root, 'global-memories'),
          AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
          AURA_E2E: '1',
          AURA_E2E_CORE_MODE: 'normal',
          AURA_P05_CLEAN_ACCEPTANCE: '1',
          AURA_TERMINAL_TEST_NO_PROFILE: '1',
          ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
        },
      });
      const electronPid = app.process().pid!;
      let core: number | undefined;
      let closed = false;
      try {
        const page = await app.firstWindow();
        await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 60_000 });
        const spawnedCore = await corePid(root);
        core = spawnedCore;
        const compatibility = await page.evaluate(async () => {
          const api = window.aura.v1;
          if (api.projectionCompatibility === undefined) throw new Error('missing metadata bridge');
          const snapshot = await api.projectionCompatibility.snapshot();
          const health = await api.getCoreHealth();
          if (!snapshot.ok || !health.ok) throw new Error('metadata snapshot unavailable');
          return {
            matches_runtime:
              snapshot.value.runtime_instance_id === health.value.runtime_instance_id,
            paused: snapshot.value.paused_projections,
          };
        });
        expect(compatibility).toEqual({ matches_runtime: true, paused: [] });
        await page.evaluate(async (selected_root) => {
          const workspace = await window.aura.v1.createWorkspace({
            display_name: 'L03 shutdown',
            selected_root,
          });
          if (!workspace.ok) throw new Error(workspace.error.message);
          const session = await window.aura.v1.createSession({
            workspace_id: workspace.value.workspace.workspace_id,
            title: 'L03 shutdown',
          });
          if (!session.ok) throw new Error(session.error.message);
        }, work);
        await page.locator('.session-row').filter({ hasText: 'L03 shutdown' }).click();
        await openTerminalTab(page);
        await expect(page.locator('.xterm')).toHaveCount(1, { timeout: 30_000 });
        await page.locator('.terminal-host').click();
        if (mode === 'escape') {
          await page.keyboard.type('Write-Out');
          await page.keyboard.press('Escape');
        } else {
          await page.keyboard.type(
            "for ($i=0; $i -lt 200000; $i++) { Write-Output ('SHUTDOWN-' + 'STREAM-' + $i); Start-Sleep -Milliseconds 1 }",
          );
          await page.keyboard.press('Enter');
          await expect(page.locator('.xterm-accessibility-tree')).toContainText(
            'SHUTDOWN-STREAM-',
            { timeout: 10_000 },
          );
        }
        const started = Date.now();
        const result = await Promise.race([
          app.close().then(() => 'closed'),
          new Promise<'timeout'>((resolve) => setTimeout(() => resolve('timeout'), 10_000)),
        ]);
        closed = result === 'closed';
        expect(result, 'closing must not wait for JS delivery of pending native I/O').toBe(
          'closed',
        );
        await expect.poll(() => alive(electronPid), { timeout: 5_000 }).toBe(false);
        await expect.poll(() => alive(spawnedCore), { timeout: 5_000 }).toBe(false);
        const evidence = info.outputPath('shutdown.json');
        await writeFile(
          evidence,
          JSON.stringify(
            {
              mode,
              repetition,
              electronPid,
              corePid: spawnedCore,
              elapsed_ms: Date.now() - started,
              root,
            },
            null,
            2,
          ),
        );
        await info.attach('confirmed-process-exit', {
          path: evidence,
          contentType: 'application/json',
        });
      } finally {
        // Only the two exact processes created above, never arbitrary existing Aura instances.
        if (!closed && (await alive(electronPid))) app.process().kill();
        core ??= await corePid(root).catch(() => undefined);
        if (core !== undefined && (await alive(core)))
          await promisify(execFile)('taskkill.exe', ['/PID', String(core), '/F']);
      }
    });
  }
}
