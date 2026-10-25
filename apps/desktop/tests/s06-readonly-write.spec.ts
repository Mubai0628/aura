import { _electron as electron, expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';
import { mkdir, mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { writeProvider } from './s06-file-write-provider.mjs';
import { armReadOnlyWrite } from './s06/setup';

/** The Electron window, whose `window.aura.v1` bridge is the product's own preload surface. */
type ElectronPage = Page;

/**
 * Real vertical chain: a ReadOnly session, a real Agent file write, a real approval.
 *
 * WHAT IS REAL HERE. Electron, the production Core, the authenticated named pipe, the real
 * permission decision, the real file service and the workspace on disk are all production
 * code. The only substituted component is the remote model service, a local HTTP server; it
 * is what lets a tool call be driven without a human typist and does not stand in for any
 * Aura behaviour under test.
 *
 * WHAT IT PROVES, IN ORDER.
 * 1. A ReadOnly session that asks to write a workspace file is **asked**, not refused — the
 *    confirmed rule is that a mode is a default rather than a ceiling.
 * 2. The file does **not** exist while that request is pending. An approval flow that had
 *    already written the file would make the dialog meaningless, so this is asserted from the
 *    real disk, not from the projection.
 * 3. The approval the panel offers names the workspace target and the real capability, so the
 *    user can judge what they are answering.
 * 4. Answering `allow_once` makes the approved write really land, with exactly the requested
 *    content. This is the half that proves the approval is authority rather than decoration.
 */
/**
 * Whether `path` is genuinely absent, rather than merely unreadable.
 *
 * The distinction matters for a negative assertion: a refused read and a file that was never
 * created both throw, and only the second means "no side effect happened". `ENOENT` is the
 * platform's own answer for "no such file", so it is the only error that supports the claim.
 */
async function absentWithEnoent(path: string): Promise<boolean> {
  try {
    await readFile(path, 'utf8');
    return false;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === 'ENOENT';
  }
}

/**
 * The run's own turn state, as the renderer's projection reports it.
 *
 * `active` is `null` once a turn is over and the session has no live run — an ended turn is the
 * strongest form of "not waiting for an answer", so it is reported as `none` rather than read as
 * a missing projection. Treating it as an error would make the finished case look like a failure
 * precisely when the product did the right thing.
 */
async function readTurnState(page: ElectronPage, sessionId: string): Promise<string> {
  return page.evaluate(async (id) => {
    const history = await window.aura.v1.readAgentHistory({ session_id: id, limit: 20 });
    if (!history.ok) return 'unavailable';
    const active = history.value.active;
    if (!active) return 'none';
    return active.turn?.state?.state ?? 'none';
  }, sessionId);
}

/**
 * Prepares the shared real chain: an isolated workspace, a ReadOnly session, the local model
 * service bound through the ordinary settings routes, and one submitted message.
 *
 * Extracted so the API-driven and panel-driven tests run the **same** production setup. Two
 * copies would be two chances for one of them to drift into a path the product does not take,
 * and the panel test's whole value is that it differs only in how the answer is given.
 */
// eslint-disable-next-line no-empty-pattern -- Electron fixtures are created explicitly.
test('S06 ReadOnly write asks, and the approved write really lands', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s06-readonly-write-'));
  const workspaceRoot = join(root, 'workspace');
  await mkdir(workspaceRoot, { recursive: true });
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });

  const target = join(workspaceRoot, 'approved-by-user.txt');
  const provider = await writeProvider({
    path: 'approved-by-user.txt',
    text: 'written only after the user approved it\n',
  });
  const app = await electron.launch({
    args: [`--user-data-dir=${join(root, 'Electron')}`, resolve(import.meta.dirname, '..')],
    env: {
      ...process.env,
      APPDATA: join(root, 'Roaming'),
      LOCALAPPDATA: root,
      AURA_MEMORY_TEST_ROOT: join(root, 'global-memories'),
      AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
      AURA_E2E: '1',
      AURA_E2E_CORE_MODE: 'normal',
      AURA_P05_CLEAN_ACCEPTANCE: '1',
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
  });
  const pageErrors: string[] = [];
  try {
    const page = await app.firstWindow();
    page.on('pageerror', (error) => pageErrors.push(error.message));
    await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 60_000 });

    // A real workspace on disk, a real session, and the real ReadOnly default.
    const setup = await page.evaluate(async (selectedRoot) => {
      const api = window.aura.v1;
      const workspace = await api.createWorkspace({
        display_name: 'S06 ReadOnly write',
        selected_root: selectedRoot,
      });
      if (!workspace.ok) return { error: `workspace: ${JSON.stringify(workspace)}` };
      const workspaceId = workspace.value.workspace.workspace_id;
      const session = await api.createSession({ workspace_id: workspaceId, title: '' });
      if (!session.ok) return { error: `session: ${JSON.stringify(session)}` };
      const sessionId = session.value.session_id;
      // The setter is revision-guarded, so the current revision is read first: that guard is
      // the real one, and a test that bypassed it would not be exercising the product path.
      const before = await api.getSessionPermissionState({ session_id: sessionId });
      if (!before.ok) return { error: `permission state: ${JSON.stringify(before)}` };
      const mode = await api.setSessionPermissionMode({
        session_id: sessionId,
        expected_revision: before.value.revision,
        mode: 'read_only',
      });
      if (!mode.ok) return { error: `mode: ${JSON.stringify(mode)}` };
      return { workspaceId, sessionId, mode: mode.value.mode };
    }, workspaceRoot);
    expect(setup.error ?? 'no error').toBe('no error');
    expect(setup.mode).toBe('read_only');

    // Bind the isolated local model service, exactly as the ordinary settings path does.
    const bound = await page.evaluate(
      async ({ url }) => {
        const api = window.aura.v1;
        const settings = await api.getModelSettings();
        if (!settings.ok) return { error: 'model settings unavailable' };
        const provider = await api.createModelProvider({
          expected_revision: settings.value.models_revision,
          input: {
            display_name: 'S06 isolated model',
            source: { type: 'custom', catalog_provider_id: null },
          },
        });
        if (!provider.ok) return { error: `provider: ${JSON.stringify(provider)}` };
        const endpoint = await api.createModelEndpoint({
          expected_revision: provider.value.revision,
          provider_id: provider.value.value.id,
          display_name: 'S06 local endpoint',
          api_format: 'open_ai_chat_completions',
          base_url: url,
          discovery_mode: { type: 'disabled' },
          chat_output_token_field: 'max_tokens',
          chat_instruction_role: 'system',
          auth_scheme: 'none',
          openai_organization_id: null,
          openai_project_id: null,
          anthropic_workspace_id: null,
          timeouts: {
            connect_ms: 5000,
            first_event_ms: 10_000,
            stream_idle_ms: 10_000,
            total_ms: 60_000,
          },
          max_parallel_model_requests: 2,
        });
        if (!endpoint.ok) return { error: `endpoint: ${JSON.stringify(endpoint)}` };
        const profile = await api.createModelProfile({
          expected_revision: endpoint.value.revision,
          input: {
            endpoint_id: endpoint.value.value.id,
            provider_model_name: 'fixture',
            display_name: 'S06 fixture model',
            notes: null,
            context_window_tokens: 256000,
          },
        });
        if (!profile.ok) return { error: `profile: ${JSON.stringify(profile)}` };
        const enabled = await api.updateModelProfile({
          expected_revision: profile.value.revision,
          model_profile_id: profile.value.value.id,
          display_name: 'S06 fixture model',
          notes: null,
          enabled: true,
          request_defaults: {
            max_output_tokens: 4096,
            temperature: null,
            top_p: null,
            chat_output_token_field: null,
            chat_instruction_role: null,
          },
          historical_snapshot_max_tokens: 1024,
        });
        if (!enabled.ok) return { error: `profile enable: ${JSON.stringify(enabled)}` };
        return { profileId: profile.value.value.id };
      },
      { url: provider.url },
    );
    info.attach('model-binding', {
      body: JSON.stringify(bound),
      contentType: 'application/json',
    });
    expect(bound.error ?? 'no error').toBe('no error');
    if (!setup.sessionId || !bound.profileId) throw new Error(JSON.stringify({ setup, bound }));

    // Make it the session's model, then ask for the write through a real submitted turn.
    const started = await page.evaluate(
      async ({ sessionId, profileId }) => {
        const api = window.aura.v1;
        // The session binding has its own CAS revision, separate from the model
        // configuration's `models_revision`: a brand-new session has never been bound, so its
        // binding revision is 0. (Confirmed against the Core handler, which compares
        // `session_model_binding_state(session_id).revision`.)
        const binding = await api.setSessionModelBinding({
          session_id: sessionId,
          expected_revision: '0',
          model_profile_id: profileId,
        });
        if (!binding.ok) return { error: `binding: ${JSON.stringify(binding)}` };
        const submitted = await api.submitAgentMessage({
          session_id: sessionId,
          destination: { lane: 'next_turn' },
          text: '请把这个文件写入工作区：approved-by-user.txt',
        });
        if (!submitted.ok) return { error: `submit: ${JSON.stringify(submitted)}` };
        // `agent.submit` returns the inbox projection, not a run id: the run identity arrives
        // with the turn projection. Returning the projection is honest about what the route says.
        return { inbox: submitted.value };
      },
      { sessionId: setup.sessionId, profileId: bound.profileId },
    );
    expect(started.error ?? 'no error').toBe('no error');
    info.attach('submit-result', {
      body: JSON.stringify(started),
      contentType: 'application/json',
    });

    // Diagnose the real state the run reached, so a failure names its cause instead of
    // only reporting that no approval appeared.
    await page.waitForTimeout(3000);
    const observed = await page.evaluate(
      async ({ sessionId }) => {
        const api = window.aura.v1;
        const history = await api.readAgentHistory({ session_id: sessionId, limit: 20 });
        return { history: history.ok ? history.value : history };
      },
      { sessionId: setup.sessionId },
    );
    info.attach('observed', { body: JSON.stringify(observed), contentType: 'application/json' });
    info.attach('provider-requests', {
      body: JSON.stringify(provider.requests.map((r) => ({ turn: r.turn, step: r.step }))),
      contentType: 'application/json',
    });

    // 1. The real permission decision asked: the Agent's own run projection names the pending
    // approval and reports the run as `awaiting_approval`. (`approval.list_pending` is the
    // separate P04 maintenance surface — a tool approval for a turn appears on the run, which
    // is exactly what the renderer reads.)
    const readRun = () =>
      page.evaluate(async (sessionId) => {
        const api = window.aura.v1;
        const history = await api.readAgentHistory({ session_id: sessionId, limit: 20 });
        if (!history.ok) return { error: history.error };
        if (!history.value.active) return { ids: [], turnState: null, runState: null };
        return {
          ids: history.value.active.pending_approval_ids,
          turnState: history.value.active.turn.state.state,
          runState: history.value.active.run.state.state,
        };
      }, setup.sessionId);

    await expect
      .poll(async () => (await readRun()).turnState, {
        timeout: 90_000,
        message: 'a ReadOnly workspace write must raise a real approval',
      })
      .toBe('awaiting_approval');
    const pending = await readRun();
    info.attach('pending', { body: JSON.stringify(pending), contentType: 'application/json' });
    if (!pending.ids?.[0]) throw new Error(JSON.stringify(pending));
    expect(pending.ids.length).toBeGreaterThan(0);

    // 2. Nothing has been written while the request waits for its answer.
    //
    // The judgment is ENOENT specifically, not "some error". Any other failure — a permissions
    // error, an EISDIR, a sharing violation — would also make `readFile` throw, and treating that
    // as "the file does not exist" would let a real effect pass as no effect. So the absence is
    // asserted from the error the platform really returns for a missing file, and anything else
    // fails the run.
    expect(await absentWithEnoent(target)).toBe(true);

    // 3. The approval names the workspace target and the capability the user is granting.
    const approval = await page.evaluate(
      async ({ sessionId, approvalId }) => {
        const result = await window.aura.v1.getAgentApproval({
          session_id: sessionId,
          approval_id: approvalId,
        });
        if (!result.ok) return { error: result.error };
        const view = result.value;
        return {
          actionKind: view.operation.content.action_kind,
          capabilities: view.operation.content.capabilities,
          resources: view.operation.content.resources.map((r) => r.scope_kind),
          allowedDecisions: view.approval.allowed_decisions,
          mode: view.mode,
          reasons: view.reasons,
          revision: view.approval.revision,
        };
      },
      { sessionId: setup.sessionId, approvalId: pending.ids[0] },
    );
    info.attach('approval-view', {
      body: JSON.stringify(approval),
      contentType: 'application/json',
    });
    expect(approval.error).toBeUndefined();
    if (!approval.revision) throw new Error(JSON.stringify(approval));
    // The request is a real workspace write, in ReadOnly, and the user is offered the precise
    // choices rather than a single all-or-nothing answer.
    expect(approval.mode).toBe('read_only');
    expect(approval.actionKind).toBe('fs.write');
    expect(approval.capabilities).toContain('workspace_file_write');
    expect(approval.resources).toContain('workspace_path');
    expect(approval.allowedDecisions).toContain('allow_once');

    // 4. The user answers, and the approved write really lands on disk. This is the half that
    // makes the approval meaningful: until now nothing was written, and after it the real file
    // exists with exactly the content the model asked to write.
    const resolved = await page.evaluate(
      async ({ sessionId, approvalId, revision }) => {
        const result = await window.aura.v1.resolveAgentApproval({
          session_id: sessionId,
          approval_id: approvalId,
          expected_revision: revision,
          decision: 'allow_once',
        });
        return result.ok ? { state: result.value.run.state } : { error: result.error };
      },
      {
        sessionId: setup.sessionId,
        approvalId: pending.ids[0],
        revision: approval.revision,
      },
    );
    info.attach('resolved', { body: JSON.stringify(resolved), contentType: 'application/json' });
    expect(resolved.error).toBeUndefined();

    await expect
      .poll(async () => readFile(target, 'utf8').catch(() => null), {
        timeout: 60_000,
        message: 'the approved write must produce the real file',
      })
      .toBe('written only after the user approved it\n');
    info.attach('file-on-disk', {
      body: await readFile(target, 'utf8'),
      contentType: 'text/plain',
    });
    expect(provider.errors).toEqual([]);
    expect(pageErrors).toEqual([]);
  } finally {
    await app.close();
    await provider.close();
  }
});

/**
 * Negative half: answering `deny` must not write, and the refusal must be honest.
 *
 * This is the counter-example against the failure mode that would make the whole approval flow
 * worthless — a "deny" that still performed the write, or that reported success. It asserts the
 * **real disk** after a real denial through the real route, and that the turn reaches a
 * terminal state rather than staying pending forever.
 */
// eslint-disable-next-line no-empty-pattern -- Electron fixtures are created explicitly.
test('S06 a denied ReadOnly write leaves the workspace untouched', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s06-denied-write-'));
  const workspaceRoot = join(root, 'workspace');
  await mkdir(workspaceRoot, { recursive: true });
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });

  const target = join(workspaceRoot, 'never-written.txt');
  const provider = await writeProvider({
    path: 'never-written.txt',
    text: 'this must never reach the disk\n',
  });
  const app = await electron.launch({
    args: [`--user-data-dir=${join(root, 'Electron')}`, resolve(import.meta.dirname, '..')],
    env: {
      ...process.env,
      APPDATA: join(root, 'Roaming'),
      LOCALAPPDATA: root,
      AURA_MEMORY_TEST_ROOT: join(root, 'global-memories'),
      AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
      AURA_E2E: '1',
      AURA_E2E_CORE_MODE: 'normal',
      AURA_P05_CLEAN_ACCEPTANCE: '1',
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
  });
  const pageErrors: string[] = [];
  try {
    const page = await app.firstWindow();
    page.on('pageerror', (error) => pageErrors.push(error.message));
    await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 60_000 });

    const armed = await page.evaluate(async (selectedRoot) => {
      const api = window.aura.v1;
      const workspace = await api.createWorkspace({
        display_name: 'S06 denied write',
        selected_root: selectedRoot,
      });
      if (!workspace.ok) return { error: `workspace: ${JSON.stringify(workspace)}` };
      const session = await api.createSession({
        workspace_id: workspace.value.workspace.workspace_id,
        title: '',
      });
      if (!session.ok) return { error: `session: ${JSON.stringify(session)}` };
      const sessionId = session.value.session_id;
      const before = await api.getSessionPermissionState({ session_id: sessionId });
      if (!before.ok) return { error: `state: ${JSON.stringify(before)}` };
      const mode = await api.setSessionPermissionMode({
        session_id: sessionId,
        expected_revision: before.value.revision,
        mode: 'read_only',
      });
      if (!mode.ok) return { error: `mode: ${JSON.stringify(mode)}` };
      const settings = await api.getModelSettings();
      if (!settings.ok) return { error: 'settings unavailable' };
      const providerRecord = await api.createModelProvider({
        expected_revision: settings.value.models_revision,
        input: {
          display_name: 'S06 denied model',
          source: { type: 'custom', catalog_provider_id: null },
        },
      });
      if (!providerRecord.ok) return { error: `provider: ${JSON.stringify(providerRecord)}` };
      return {
        sessionId,
        providerId: providerRecord.value.value.id,
        revision: providerRecord.value.revision,
      };
    }, workspaceRoot);
    expect(armed.error ?? 'no error').toBe('no error');
    if (!armed.providerId || !armed.revision) throw new Error(JSON.stringify(armed));

    const wired = await page.evaluate(
      async ({ providerId, revision, url }) => {
        const api = window.aura.v1;
        const endpoint = await api.createModelEndpoint({
          expected_revision: revision,
          provider_id: providerId,
          display_name: 'S06 denied endpoint',
          api_format: 'open_ai_chat_completions',
          base_url: url,
          discovery_mode: { type: 'disabled' },
          chat_output_token_field: 'max_tokens',
          chat_instruction_role: 'system',
          auth_scheme: 'none',
          openai_organization_id: null,
          openai_project_id: null,
          anthropic_workspace_id: null,
          timeouts: {
            connect_ms: 5000,
            first_event_ms: 10_000,
            stream_idle_ms: 10_000,
            total_ms: 60_000,
          },
          max_parallel_model_requests: 2,
        });
        if (!endpoint.ok) return { error: `endpoint: ${JSON.stringify(endpoint)}` };
        const profile = await api.createModelProfile({
          expected_revision: endpoint.value.revision,
          input: {
            endpoint_id: endpoint.value.value.id,
            provider_model_name: 'fixture',
            display_name: 'S06 denied model profile',
            notes: null,
            context_window_tokens: 256000,
          },
        });
        if (!profile.ok) return { error: `profile: ${JSON.stringify(profile)}` };
        const enabled = await api.updateModelProfile({
          expected_revision: profile.value.revision,
          model_profile_id: profile.value.value.id,
          display_name: 'S06 denied model profile',
          notes: null,
          enabled: true,
          request_defaults: {
            max_output_tokens: 4096,
            temperature: null,
            top_p: null,
            chat_output_token_field: null,
            chat_instruction_role: null,
          },
          historical_snapshot_max_tokens: 1024,
        });
        if (!enabled.ok) return { error: `enable: ${JSON.stringify(enabled)}` };
        return { profileId: profile.value.value.id };
      },
      { providerId: armed.providerId, revision: armed.revision, url: provider.url },
    );
    expect(wired.error ?? 'no error').toBe('no error');
    if (!armed.sessionId || !wired.profileId) throw new Error(JSON.stringify({ armed, wired }));

    const started = await page.evaluate(
      async ({ sessionId, profileId }) => {
        const api = window.aura.v1;
        const binding = await api.setSessionModelBinding({
          session_id: sessionId,
          expected_revision: '0',
          model_profile_id: profileId,
        });
        if (!binding.ok) return { error: `binding: ${JSON.stringify(binding)}` };
        const submitted = await api.submitAgentMessage({
          session_id: sessionId,
          destination: { lane: 'next_turn' },
          text: '请写入 never-written.txt',
        });
        return submitted.ok ? {} : { error: `submit: ${JSON.stringify(submitted)}` };
      },
      { sessionId: armed.sessionId, profileId: wired.profileId },
    );
    expect(started.error ?? 'no error').toBe('no error');

    const readRun = () =>
      page.evaluate(async (sessionId) => {
        const history = await window.aura.v1.readAgentHistory({ session_id: sessionId, limit: 20 });
        if (!history.ok) return { error: history.error };
        if (!history.value.active) return { ids: [], turnState: null };
        return {
          ids: history.value.active.pending_approval_ids,
          turnState: history.value.active.turn.state.state,
        };
      }, armed.sessionId);

    await expect
      .poll(async () => (await readRun()).turnState, { timeout: 90_000 })
      .toBe('awaiting_approval');
    const pending = await readRun();
    if (!pending.ids?.[0]) throw new Error(JSON.stringify(pending));

    const view = await page.evaluate(
      async ({ sessionId, approvalId }) => {
        const result = await window.aura.v1.getAgentApproval({
          session_id: sessionId,
          approval_id: approvalId,
        });
        return result.ok ? { revision: result.value.approval.revision } : { error: result.error };
      },
      { sessionId: armed.sessionId, approvalId: pending.ids[0] },
    );
    expect(view.error).toBeUndefined();
    if (!view.revision) throw new Error(JSON.stringify(view));

    await page.evaluate(
      async ({ sessionId, approvalId, revision }) => {
        await window.aura.v1.resolveAgentApproval({
          session_id: sessionId,
          approval_id: approvalId,
          expected_revision: revision,
          decision: 'deny',
        });
      },
      { sessionId: armed.sessionId, approvalId: pending.ids[0], revision: view.revision },
    );

    // The turn must leave the waiting state, and the file must never appear — judged by ENOENT,
    // so a refused read cannot stand in for "nothing was written".
    await expect
      .poll(async () => (await readRun()).turnState, { timeout: 60_000 })
      .not.toBe('awaiting_approval');
    expect(
      await absentWithEnoent(target),
      'a denied write must not reach the disk (and must be absent, not merely unreadable)',
    ).toBe(true);
    expect(provider.errors).toEqual([]);
    expect(pageErrors).toEqual([]);
  } finally {
    await app.close();
    await provider.close();
  }
});

/**
 * The same chain answered through the **real panel**, by a real click.
 *
 * The tests above answer through `window.aura.v1.resolveAgentApproval`, which is the real route
 * but not the real user path. The difference matters for the read-only claim: a product where the
 * panel renders the wrong target, offers no answer, or answers a different question than the one
 * shown would still pass an API-driven test. So this drives the actual rendered control and
 * asserts the effect on the real disk afterwards.
 *
 * It also exercises the half that only exists in the UI: the decision the panel offers is the
 * decision the user can take, and after taking it the run must actually continue — an approval
 * flow that resolved but never resumed the Agent would leave the user with a dialog that lies.
 */
// eslint-disable-next-line no-empty-pattern -- Electron fixtures are created explicitly.
test('S06 the rendered approval panel really answers the write', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s06-panel-click-'));
  const workspaceRoot = join(root, 'workspace');
  await mkdir(workspaceRoot, { recursive: true });
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });

  const target = join(workspaceRoot, 'clicked-approval.txt');
  const provider = await writeProvider({
    path: 'clicked-approval.txt',
    text: 'written because the user clicked the panel\n',
  });
  const app = await electron.launch({
    args: [`--user-data-dir=${join(root, 'Electron')}`, resolve(import.meta.dirname, '..')],
    env: {
      ...process.env,
      APPDATA: join(root, 'Roaming'),
      LOCALAPPDATA: root,
      AURA_MEMORY_TEST_ROOT: join(root, 'global-memories'),
      AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
      AURA_E2E: '1',
      AURA_E2E_CORE_MODE: 'normal',
      AURA_P05_CLEAN_ACCEPTANCE: '1',
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
  });
  const pageErrors: string[] = [];
  try {
    const page = await app.firstWindow();
    page.on('pageerror', (error) => pageErrors.push(error.message));
    await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 60_000 });

    const armed = await armReadOnlyWrite(page, workspaceRoot, provider.url, {
      title: 'S06 panel click',
      text: '请把这个文件写入工作区：clicked-approval.txt',
    });
    expect(armed.error ?? 'no error').toBe('no error');

    // The panel appears on its own, from the run projection the renderer reads.
    const panel = page.locator('section[aria-label="当前任务审批"]');
    await expect(panel).toBeVisible({ timeout: 90_000 });
    info.attach('panel-visible', { body: await panel.innerText(), contentType: 'text/plain' });

    // Nothing is written while the panel waits.
    expect(
      await absentWithEnoent(target),
      'the file must not exist while the panel is still asking',
    ).toBe(true);

    // The one answer this test takes, by its real rendered label.
    const allowOnce = panel.getByRole('button', { name: '仅此一次' });
    await expect(allowOnce).toBeEnabled({ timeout: 30_000 });
    await allowOnce.click();

    // The click really resolved the question and the approved write really landed.
    await expect
      .poll(async () => readFile(target, 'utf8').catch(() => null), {
        timeout: 60_000,
        message: 'the write approved by the panel click must reach the disk',
      })
      .toBe('written because the user clicked the panel\n');
    info.attach('file-on-disk', {
      body: await readFile(target, 'utf8'),
      contentType: 'text/plain',
    });

    // And the run moved on rather than staying on a question that was already answered.
    await expect
      .poll(async () => readTurnState(page, armed.sessionId as string), { timeout: 60_000 })
      .not.toBe('awaiting_approval');

    expect(provider.errors).toEqual([]);
    expect(pageErrors).toEqual([]);
  } finally {
    await app.close();
    await provider.close();
  }
});
