import { expect, test, type Page, type TestInfo } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { SessionId, TerminalOpeningChangeV1, TerminalViewV1 } from '@aura/contracts/generated';
import type { ResultEnvelope } from '../shared/aura-api';
import { launch } from './s06/harness';
import { chooseDefaultTerminal, openTerminalTab } from './l10-8/terminal-tabs';

interface ColdOpeningObserver {
  started: number;
  changes: { elapsed_ms: number; change: TerminalOpeningChangeV1 }[];
  detach: () => void;
}
declare global {
  interface Window {
    l03ColdOpeningObserver?: ColdOpeningObserver;
  }
}

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'aura-l03-cold-cmd-'));
  const work = join(root, 'workspace');
  await mkdir(work);
  const { app, page } = await launch(root, { AURA_TERMINAL_TEST_NO_PROFILE: '1' });
  const setup = await page.evaluate(async (selected_root) => {
    const workspace = await window.aura.v1.createWorkspace({
      display_name: 'L03 cold CMD',
      selected_root,
    });
    if (!workspace.ok) throw new Error(workspace.error.message);
    const session = await window.aura.v1.createSession({
      workspace_id: workspace.value.workspace.workspace_id,
      title: 'L03 cold CMD',
    });
    if (!session.ok) throw new Error(session.error.message);
    // Sample facts without waiting for the background PowerShell verification to finish.
    const readiness = await window.aura.v1.getRuntimeReadiness();
    const health = await window.aura.v1.getCoreHealth();
    return { session: session.value.session_id, readiness, health };
  }, work);
  await page.evaluate(() => {
    const state: ColdOpeningObserver = {
      started: performance.now(),
      changes: [],
      detach: () => {},
    };
    state.detach = window.aura.v1.terminal!.onOpening((change) => {
      state.changes.push({ elapsed_ms: performance.now() - state.started, change });
    });
    window.l03ColdOpeningObserver = state;
  });
  return { app, page, root, setup };
}

async function evidence(
  page: Page,
  info: TestInfo,
  root: string,
  session: SessionId,
  initial: unknown,
  original?: ResultEnvelope<TerminalViewV1>,
) {
  const facts = await page
    .evaluate(async (session_id) => {
      const state = window.l03ColdOpeningObserver;
      const terminals = await window.aura.v1.terminal!.list({ session_id });
      const health = await window.aura.v1.getCoreHealth();
      const readiness = await window.aura.v1.getRuntimeReadiness();
      return {
        elapsed_ms: state ? performance.now() - state.started : null,
        phases: state?.changes ?? [],
        terminal_ui_alerts: [...document.querySelectorAll('.terminal-panel [role="alert"]')].map(
          (element) => element.textContent,
        ),
        terminal_ui_status: [...document.querySelectorAll('.terminal-panel [role="status"]')].map(
          (element) => element.textContent,
        ),
        terminals,
        health,
        readiness,
      };
    }, session)
    .catch(() => ({ evidence_unavailable: true }));
  await info.attach('cold-cmd-original-envelope-and-ui-phase', {
    body: JSON.stringify({ root, initial, original, ...facts }, null, 2),
    contentType: 'application/json',
  });
  await page.evaluate(() => window.l03ColdOpeningObserver?.detach()).catch(() => undefined);
}

test('L03 B0 cold CMD bridge: original envelope and phases prove the first native open', async ({
  browserName,
}, info) => {
  void browserName;
  test.setTimeout(120_000);
  const f = await fixture();
  let original: ResultEnvelope<TerminalViewV1> | undefined;
  try {
    original = await f.page.evaluate(
      ({ session_id, opening_id }) =>
        window.aura.v1.terminal!.beginOpen({
          contract_version: 1,
          opening_id,
          request: { session_id, shell: 'cmd', columns: 80, rows: 24 },
        }),
      { session_id: f.setup.session, opening_id: randomUUID() },
    );
    expect(original.ok, JSON.stringify(original)).toBe(true);
    if (!original.ok) throw new Error(original.error.message);
    expect(original.value).toMatchObject({
      session_id: f.setup.session,
      shell: 'cmd',
      owner: 'user',
      controller: 'user',
      end: null,
    });
    const closed = await f.page.evaluate(
      ({ session_id, terminal_id }) => window.aura.v1.terminal!.close({ session_id, terminal_id }),
      { session_id: f.setup.session, terminal_id: original.value.terminal_id },
    );
    expect(closed.ok, JSON.stringify(closed)).toBe(true);
  } finally {
    try {
      await evidence(f.page, info, f.root, f.setup.session, f.setup, original);
    } finally {
      await f.app.close();
    }
  }
});

test('L03 B0 cold CMD UI: first open keeps its phase and alert evidence through rendering', async ({
  browserName,
}, info) => {
  void browserName;
  test.setTimeout(120_000);
  const f = await fixture();
  try {
    await f.app.evaluate(({ BrowserWindow }) =>
      BrowserWindow.getAllWindows()[0]?.setSize(1800, 800),
    );
    await f.page.locator('.session-row').filter({ hasText: 'L03 cold CMD' }).click();
    await chooseDefaultTerminal(f.page, 'CMD');
    await openTerminalTab(f.page);
    await expect(f.page.locator('.xterm')).toHaveCount(1, { timeout: 60_000 });
    await f.page.getByRole('button', { name: '关闭终端 1', exact: true }).click();
    await expect(f.page.locator('.xterm')).toHaveCount(0);
  } finally {
    try {
      await evidence(f.page, info, f.root, f.setup.session, f.setup);
    } finally {
      await f.app.close();
    }
  }
});
