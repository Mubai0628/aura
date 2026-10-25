import { _electron as electron, expect } from '@playwright/test';
import type { Page } from '@playwright/test';
import type { ApprovalDecisionKind, PermissionMode } from '@aura/contracts/generated';
import { readdir, readFile } from 'node:fs/promises';
import { DatabaseSync } from 'node:sqlite';
import { join, resolve } from 'node:path';

export async function launch(
  root: string,
  extraEnv: Record<string, string> = {},
  desktopRoot = resolve(import.meta.dirname, '../..'),
  launchArgs: string[] = [],
) {
  const app = await electron.launch({
    args: [...launchArgs, `--user-data-dir=${join(root, 'Electron')}`, desktopRoot],
    env: {
      ...process.env,
      APPDATA: join(root, 'Roaming'),
      LOCALAPPDATA: join(root, 'Local'),
      AURA_MEMORY_TEST_ROOT: join(root, 'global-memories'),
      // A TEMP stand-in for the user profile, so its `.agents` folder is never the real one.
      AURA_USER_PROFILE_TEST_ROOT: join(root, 'user-profile'),
      AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
      AURA_E2E: '1',
      AURA_E2E_CORE_MODE: 'normal',
      AURA_P05_CLEAN_ACCEPTANCE: '1',
      // A terminal tab opens a terminal at once; a scenario never loads the owner's profile.
      AURA_TERMINAL_TEST_NO_PROFILE: '1',
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
      ...extraEnv,
    },
  });
  const page = await app.firstWindow();
  await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 60_000 });
  return { app, page };
}

export async function history(page: Page, session: string) {
  return page.evaluate(async (session_id) => {
    const result = await window.aura.v1.readAgentHistory({ session_id, limit: 20 });
    if (!result.ok) throw new Error(JSON.stringify(result.error));
    return result.value;
  }, session);
}

export async function finished(page: Page, session: string) {
  await expect
    .poll(
      async () => {
        const view = await history(page, session);
        return view.active?.turn.state.state ?? view.turns[0]?.turn.state.state;
      },
      { timeout: 90_000 },
    )
    .toBe('completed');
}

export async function pending(page: Page, session: string) {
  await expect
    .poll(async () => (await history(page, session)).active?.pending_approval_ids.length ?? 0, {
      timeout: 90_000,
    })
    .toBeGreaterThan(0);
  const id = (await history(page, session)).active!.pending_approval_ids[0]!;
  return page.evaluate(
    async ({ session_id, approval_id }) => {
      const result = await window.aura.v1.getAgentApproval({ session_id, approval_id });
      if (!result.ok) throw new Error(JSON.stringify(result.error));
      return result.value;
    },
    { session_id: session, approval_id: id },
  );
}

export async function answer(page: Page, session: string, decision: ApprovalDecisionKind) {
  const view = await pending(page, session);
  const result = await page.evaluate(
    async ({ session_id, approval_id, expected_revision, decision }) =>
      window.aura.v1.resolveAgentApproval({ session_id, approval_id, expected_revision, decision }),
    {
      session_id: session,
      approval_id: view.approval.approval_id,
      expected_revision: view.approval.revision,
      decision,
    },
  );
  expect(result.ok, JSON.stringify(result)).toBe(true);
  return view;
}

export async function submit(page: Page, session: string, text: string) {
  const previous = await history(page, session);
  const previousId = previous.active?.turn.turn_task_id ?? previous.turns[0]?.turn.turn_task_id;
  const result = await page.evaluate(
    async ({ session_id, text }) =>
      window.aura.v1.submitAgentMessage({ session_id, text, destination: { lane: 'next_turn' } }),
    { session_id: session, text },
  );
  expect(result.ok, JSON.stringify(result)).toBe(true);
  await expect
    .poll(
      async () => {
        const next = await history(page, session);
        const id = next.active?.turn.turn_task_id ?? next.turns[0]?.turn.turn_task_id;
        return Boolean(id && id !== previousId);
      },
      { timeout: 90_000 },
    )
    .toBe(true);
}

export async function changeMode(page: Page, session: string, mode: PermissionMode) {
  const result = await page.evaluate(
    async ({ session_id, mode }) => {
      const previous = await window.aura.v1.getSessionPermissionState({ session_id });
      if (!previous.ok) return previous;
      return window.aura.v1.setSessionPermissionMode({
        session_id,
        expected_revision: previous.value.revision,
        mode,
      });
    },
    { session_id: session, mode },
  );
  expect(result.ok, JSON.stringify(result)).toBe(true);
}

export async function hash(page: Page, workspace: string, session: string, path: string) {
  const result = await page.evaluate(async (input) => window.aura.v1.hashWorkspaceFile(input), {
    workspace_id: workspace,
    session_id: session,
    target: { path },
  });
  expect(result.ok, JSON.stringify(result)).toBe(true);
  if (!result.ok) throw new Error('hash unavailable');
  return result.value.disk_hash ?? result.value.hash;
}

export async function fileGrants(page: Page, session: string) {
  const result = await page.evaluate(
    async (session_id) =>
      window.aura.v1.listFileSessionConsents({ session_id, include_inactive: true }),
    session,
  );
  expect(result.ok, JSON.stringify(result)).toBe(true);
  if (!result.ok) throw new Error('file grants unavailable');
  return result.value;
}

export async function absent(path: string) {
  try {
    await readFile(path);
    return false;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return true;
    throw error;
  }
}

/** Read-only evidence from this case's isolated production journal. */
export async function attemptCount(root: string, operation: string) {
  const files = await readdir(root, { recursive: true });
  const matches = files.filter((file) => file.endsWith('aura-v2.db'));
  expect(matches).toHaveLength(1);
  const db = new DatabaseSync(join(root, matches[0]!), { readOnly: true });
  try {
    const row = db
      .prepare('SELECT COUNT(*) AS n FROM execution_attempts WHERE operation_id=?')
      .get(operation);
    return Number(row!.n);
  } finally {
    db.close();
  }
}
