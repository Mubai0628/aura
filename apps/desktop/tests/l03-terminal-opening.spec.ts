import { expect, test, type Page } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, rmdir, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import type { TerminalOpeningChangeV1, TerminalViewV1 } from '@aura/contracts/generated';
import type { ResultEnvelope } from '../shared/aura-api';
import { launch } from './s06/harness';

interface OpeningEvidence {
  changes: TerminalOpeningChangeV1[];
  original?: ResultEnvelope<TerminalViewV1>;
  detach: () => void;
}
declare global {
  interface Window {
    l03OpeningEvidence?: OpeningEvidence;
  }
}

async function terminals(page: Page, session_id: string) {
  return page.evaluate(async (session_id) => {
    const result = await window.aura.v1.terminal!.list({ session_id });
    if (!result.ok) throw new Error(result.error.message);
    return result.value.terminals;
  }, session_id);
}

test('real opening bridge cancels before launch, preserves window/session ownership, and opens after release', async ({
  browserName,
}, info) => {
  void browserName;
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l03-opening-'));
  const gate = await mkdtemp(join(tmpdir(), 'aura-terminal-opening-'));
  const work = join(root, 'workspace');
  await mkdir(work);
  await writeFile(join(gate, 'hold'), '');
  const { app, page } = await launch(root, {
    AURA_TERMINAL_TEST_NO_PROFILE: '1',
    AURA_TERMINAL_TEST_OPENING_GATE: gate,
  });
  let otherWindowId: number | undefined;
  try {
    const setup = await page.evaluate(async (selected_root) => {
      const workspace = await window.aura.v1.createWorkspace({
        display_name: 'L03 opening',
        selected_root,
      });
      if (!workspace.ok) throw new Error(workspace.error.message);
      const sessions = [];
      for (const title of ['L03 opening cancelled', 'L03 opening released']) {
        const result = await window.aura.v1.createSession({
          workspace_id: workspace.value.workspace.workspace_id,
          title,
        });
        if (!result.ok) throw new Error(result.error.message);
        sessions.push(result.value.session_id);
      }
      const health = await window.aura.v1.getCoreHealth();
      if (!health.ok) throw new Error(health.error.message);
      return { sessions, runtime: health.value.runtime_instance_id };
    }, work);
    const cancelledSession = setup.sessions[0]!;
    const releasedSession = setup.sessions[1]!;
    // Separate the native-launch gate from the bounded RPC budget of the shared warm check.
    await expect
      .poll(
        () =>
          page.evaluate(async () => {
            const readiness = await window.aura.v1.getRuntimeReadiness();
            if (!readiness.ok) throw new Error(readiness.error.message);
            return readiness.value.backends.find(
              (backend) => backend.backend === 'windows-user-compat-v1',
            )?.readiness.readiness;
          }),
        { timeout: 60_000 },
      )
      .toBe('ready');
    const firstToken = randomUUID();
    await page.evaluate(
      ({ opening_id, session_id }) => {
        const state = window;
        const api = window.aura.v1.terminal!;
        state.l03OpeningEvidence = {
          changes: [],
          detach: api.onOpening((change) => state.l03OpeningEvidence!.changes.push(change)),
        };
        void api
          .beginOpen({
            contract_version: 1,
            opening_id,
            request: { session_id, shell: 'power_shell', columns: 80, rows: 24 },
          })
          .then((result) => {
            state.l03OpeningEvidence!.original = result;
          });
      },
      { opening_id: firstToken, session_id: cancelledSession },
    );
    // The test feature holds after the shared runtime verification and before native work starts.
    await expect
      .poll(() => readFile(join(gate, 'entered'), 'utf8').catch(() => ''), { timeout: 60_000 })
      .toBe(cancelledSession);
    await expect
      .poll(() =>
        page.evaluate(() => window.l03OpeningEvidence!.changes.map((change) => change.phase)),
      )
      .toContain('waiting_launch');
    expect(await terminals(page, cancelledSession)).toEqual([]);
    const unknown = await page.evaluate(
      (opening_id) => window.aura.v1.terminal!.cancelOpen({ contract_version: 1, opening_id }),
      randomUUID(),
    );
    expect(unknown).toEqual({ ok: true, value: 'unknown_request' });

    // A second, sandboxed window has the public preload but is not the trusted Aura frame.
    const otherPageReady = app.waitForEvent('window');
    otherWindowId = await app.evaluate(
      async ({ BrowserWindow }, preload) => {
        const first = BrowserWindow.getAllWindows()[0]!;
        const other = new BrowserWindow({
          show: false,
          webPreferences: {
            contextIsolation: true,
            nodeIntegration: false,
            sandbox: true,
            preload,
          },
        });
        await other.loadURL(first.webContents.getURL());
        return other.id;
      },
      resolve(import.meta.dirname, '../dist/preload/index.cjs'),
    );
    const otherPage = await otherPageReady;
    const foreign = await otherPage.evaluate(async (opening_id) => {
      const state = window;
      const api = window.aura.v1.terminal!;
      state.l03OpeningEvidence = {
        changes: [],
        detach: api.onOpening((change) => state.l03OpeningEvidence!.changes.push(change)),
      };
      return api.cancelOpen({ contract_version: 1, opening_id });
    }, firstToken);
    expect(foreign.ok).toBe(false);
    expect(await page.evaluate(() => window.l03OpeningEvidence!.original)).toBeUndefined();

    const cancellation = await page.evaluate(
      (opening_id) => window.aura.v1.terminal!.cancelOpen({ contract_version: 1, opening_id }),
      firstToken,
    );
    expect(cancellation).toEqual({ ok: true, value: 'accepted' });
    await expect
      .poll(() => page.evaluate(() => window.l03OpeningEvidence!.original !== undefined))
      .toBe(true);
    const cancelled = await page.evaluate(() => window.l03OpeningEvidence!.original!);
    expect(cancelled).toMatchObject({
      ok: false,
      error: { code: 'terminal.open_cancelled', terminal: 'cancelled' },
    });
    if (!cancelled.ok)
      expect(cancelled.error.details?.outcome_recovery).not.toBe(
        'query_runtime.command_outcome_without_replay',
      );
    expect(await terminals(page, cancelledSession)).toEqual([]);
    expect(await terminals(page, releasedSession)).toEqual([]);
    expect(await otherPage.evaluate(() => window.l03OpeningEvidence!.changes)).toEqual([]);
    const cancelledChanges = await page.evaluate(() => window.l03OpeningEvidence!.changes);
    expect(cancelledChanges.length).toBeGreaterThan(0);
    expect(
      cancelledChanges.every(
        (change) =>
          change.opening_id === firstToken &&
          change.session_id === cancelledSession &&
          change.runtime_instance_id === setup.runtime,
      ),
    ).toBe(true);
    expect(cancelledChanges.some((change) => change.phase === 'launching')).toBe(false);

    await writeFile(join(gate, 'release'), '');
    const secondToken = randomUUID();
    const released = await page.evaluate(
      ({ opening_id, session_id }) =>
        window.aura.v1.terminal!.beginOpen({
          contract_version: 1,
          opening_id,
          request: { session_id, shell: 'power_shell', columns: 80, rows: 24 },
        }),
      { opening_id: secondToken, session_id: releasedSession },
    );
    expect(released.ok, JSON.stringify(released)).toBe(true);
    if (!released.ok) throw new Error(released.error.message);
    expect(released.value).toMatchObject({
      session_id: releasedSession,
      owner: 'user',
      controller: 'user',
      shell: 'power_shell',
      end: null,
    });
    expect(released.value.process_id).toBeGreaterThan(0);
    const wrongSessionClose = await page.evaluate(
      ({ session_id, terminal_id }) => window.aura.v1.terminal!.close({ session_id, terminal_id }),
      { session_id: cancelledSession, terminal_id: released.value.terminal_id },
    );
    expect(wrongSessionClose.ok).toBe(false);
    expect((await terminals(page, releasedSession))[0]?.end).toBeNull();
    const closed = await page.evaluate(
      ({ session_id, terminal_id }) => window.aura.v1.terminal!.close({ session_id, terminal_id }),
      { session_id: releasedSession, terminal_id: released.value.terminal_id },
    );
    expect(closed.ok, JSON.stringify(closed)).toBe(true);
    if (closed.ok) expect(closed.value.end?.reason).toBe('user_closed');
    expect(await terminals(page, releasedSession)).toEqual([]);
    await info.attach('real-terminal-opening', {
      body: JSON.stringify({ gate, cancelled, cancelledChanges, released, cancellation }, null, 2),
      contentType: 'application/json',
    });
  } finally {
    // Release only this test's gate before stopping the isolated Core, including failure paths.
    await writeFile(join(gate, 'release'), '');
    try {
      await page.evaluate(() => window.l03OpeningEvidence?.detach()).catch(() => undefined);
      if (otherWindowId !== undefined)
        await app.evaluate(
          ({ BrowserWindow }, id) => BrowserWindow.fromId(id)?.destroy(),
          otherWindowId,
        );
      await app.close();
    } finally {
      for (const name of ['hold', 'entered', 'release'])
        await rm(join(gate, name), { force: true });
      await rmdir(gate);
    }
  }
});
