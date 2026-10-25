/* global document, window, MutationObserver */
// Production-window helpers. Setup may use the normal configuration bridge;
// session creation, model choice and every message use the actual UI.
import assert from 'node:assert/strict';
import { _electron as electron } from '@playwright/test';
import { mkdir, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

export const desktop = resolve(import.meta.dirname, '..');
export const marker = 'AURA-R10-FILE-7Q3';

export async function harness(label, { realData = false, capture = true } = {}) {
  const stamp = new Date().toISOString().replaceAll(/[:.]/gu, '-');
  const output = resolve(desktop, '../../target', label, `${stamp}-${process.pid}`);
  const root = join(output, 'synthetic-workspace');
  for (const dir of [root, 'Electron', 'Local', 'Roaming'].map((v) => resolve(output, v)))
    await mkdir(dir, { recursive: true });
  await writeFile(join(root, 'note.txt'), `Synthetic verification marker: ${marker}\n`, 'utf8');
  await writeFile(join(root, '中文文件.txt'), '这是一份合成测试文件。\n', 'utf8');
  await mkdir(join(root, 'empty'));
  const env = { ...process.env, AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1' };
  delete env.AURA_IPC_TRACE_FILE;
  delete env.AURA_RENDERER_URL;
  if (!realData)
    Object.assign(env, { LOCALAPPDATA: join(output, 'Local'), APPDATA: join(output, 'Roaming') });
  const errors = [];
  const report = { output, realData, errors, checks: [] };
  let app;
  let page;
  console.log(`Evidence: ${output}`);
  try {
    app = await electron.launch({
      args: [`--user-data-dir=${join(output, 'Electron')}`, desktop],
      cwd: desktop,
      env,
      timeout: 60_000,
    });
    page = await app.firstWindow();
    page.on('pageerror', (error) => errors.push(error.message.slice(0, 300)));
    await page.waitForLoadState('domcontentloaded');
    const nativeWindow = await app.browserWindow(page);
    await poll(
      () => nativeWindow.evaluate((window) => window.isVisible() && !window.isMinimized()),
      Boolean,
      30_000,
      'actual desktop window is visible',
    );
    report.windowVisible = true;
    await poll(
      async () =>
        page.evaluate(async () => {
          const result = await window.aura?.v1.getCoreHealth();
          return result?.ok && result.value.payload.state === 'ready';
        }),
      Boolean,
      60_000,
      'Core readiness',
    );
    const name = `R10 ${stamp}`;
    const workspace = await api(page, 'createWorkspace', {
      display_name: name,
      selected_root: root,
    });
    const workspaceId = workspace.workspace.workspace_id;
    await page.getByRole('button', { name: `在 ${name} 新建会话`, exact: true }).waitFor();
    report.workspaceId = workspaceId;
    return {
      app,
      page,
      output,
      root,
      name,
      workspaceId,
      report,
      record(check, value) {
        report.checks.push({ check, value });
        console.log(`${check}: ${JSON.stringify(value)}`);
      },
      async finish(error) {
        report.passed = !error && errors.length === 0;
        if (error) report.error = String(error.stack ?? error).slice(0, 2500);
        if (capture)
          await page
            .screenshot({
              path: join(output, report.passed ? 'settled.png' : 'failure.png'),
              fullPage: true,
            })
            .catch(() => {});
        await writeFile(join(output, 'evidence.json'), JSON.stringify(report, null, 2));
        await app.close();
        if (!report.passed) throw error ?? new Error(errors.join('\n'));
      },
    };
  } catch (error) {
    report.passed = false;
    report.error = String(error.stack ?? error).slice(0, 2500);
    if (capture)
      await page?.screenshot({ path: join(output, 'startup-failure.png') }).catch(() => {});
    await writeFile(join(output, 'evidence.json'), JSON.stringify(report, null, 2));
    await app?.close().catch(() => {});
    throw error;
  }
}

export async function api(page, method, input) {
  return page.evaluate(
    async ({ method, input }) => {
      const result = await window.aura.v1[method](input);
      if (!result.ok) throw new Error(`${method}: ${result.error.message}`);
      return result.value;
    },
    { method, input },
  );
}

export async function poll(read, valid, timeout = 30_000, label = 'expected state') {
  const end = Date.now() + timeout;
  let value;
  while (Date.now() < end) {
    value = await read();
    if (valid(value)) return value;
    await new Promise((done) => setTimeout(done, 150));
  }
  // Never stringify arbitrary history/provider data into a timeout message.
  const last = ['number', 'boolean'].includes(typeof value) ? `; last value: ${value}` : '';
  throw new Error(`Timed out: ${label}${last}`);
}

export const history = (page, session) =>
  api(page, 'readAgentHistory', {
    session_id: session,
    before_turn_task_id: null,
    within_turn: null,
    limit: 25,
  });

export async function sendNew(h, modelName, text) {
  await h.page.getByRole('button', { name: `在 ${h.name} 新建会话`, exact: true }).click();
  await h.page.locator('.composer textarea').fill(text);
  assert.equal(
    await h.page.locator('.conversation-header h2').innerText(),
    '新会话',
    'draft must not rename before send',
  );
  await h.page.locator('.model-select').click();
  await h.page.getByRole('menuitemradio').filter({ hasText: modelName }).first().click();
  await h.page.getByRole('button', { name: '发送消息', exact: true }).click();
  const title = Array.from(text).slice(0, 5).join('');
  const sessions = await poll(
    () => api(h.page, 'getWorkspaceSessions'),
    (v) => v.payload.sessions.some((s) => s.workspace_id === h.workspaceId && s.title === title),
    30_000,
    'first-send automatic title',
  );
  const session = sessions.payload.sessions.find(
    (s) => s.workspace_id === h.workspaceId && s.title === title,
  );
  await h.page.locator(`.agent-conversation[data-session-id="${session.session_id}"]`).waitFor();
  assert.equal(await h.page.locator('.session-row[aria-current="page"]').innerText(), title);
  h.record('real composer created, named and opened session', {
    sessionId: session.session_id,
    title,
  });
  return session.session_id;
}

export async function sendNext(h, text) {
  await h.page.locator('.composer textarea').fill(text);
  await h.page.getByRole('button', { name: '发送消息', exact: true }).click();
}

/** Types into the live composer and uses the addition control, not the next-turn send. */
export async function sendAddition(h, text) {
  await h.page.getByRole('button', { name: '追加到当前任务', exact: true }).waitFor();
  await h.page.locator('.composer textarea').fill(text);
  await h.page.getByRole('button', { name: '追加到当前任务', exact: true }).click();
}

export async function completed(h, session, count = 1, timeout = 60_000) {
  const view = await poll(
    () => history(h.page, session),
    (v) => {
      const state = v.turns.at(-1)?.turn.state.state;
      if (v.turns.length >= count && ['failed', 'recovery_required', 'cancelled'].includes(state))
        throw new Error(`Turn ended unexpectedly: ${state}`);
      return v.turns.length >= count && state === 'completed';
    },
    timeout,
    'automatic completed turn',
  );
  assert.equal(view.inbox.paused, false, 'ordinary send must start without Continue Queue');
  assert.equal(view.inbox.messages.length, 0, 'queue drained');
  const id = view.turns.at(-1).turn.turn_task_id;
  await h.page
    .locator(`[data-turn-id="${id}"] .agent-turn-status[data-state="completed"]`)
    .waitFor();
  assert.equal(await h.page.locator(`[data-turn-id="${id}"] .agent-turn-status`).count(), 1);
  // History carries a bounded preview and deliberately blanks state.final_report.
  // Read the authoritative completion through its ordinary paged text route.
  let finalReport = '';
  let offset = 0;
  for (;;) {
    const part = await api(h.page, 'readAgentMessageText', {
      session_id: session,
      source: { kind: 'completion', turn_task_id: id },
      offset,
      limit: 16_384,
    });
    assert.equal(part.from_offset, offset, 'sealed completion pages must retain their origin');
    finalReport += part.text;
    if (part.next_offset === null) break;
    assert.ok(part.next_offset > offset, 'a real next page must make progress');
    offset = part.next_offset;
  }
  h.record('exact completion read through the authenticated text route', {
    turnTaskId: id,
    bytes: Buffer.byteLength(finalReport),
  });
  await poll(
    () => h.page.locator(`[data-turn-id="${id}"] .agent-final-report`).innerText(),
    (text) => text === finalReport,
    5_000,
    'exact rendered final report',
  );
  assert.equal(
    await h.page.locator(`[data-turn-id="${id}"] .agent-final-report`).innerText(),
    finalReport,
  );
  return view;
}

export async function configureFixture(page, url) {
  const settings = await api(page, 'getModelSettings');
  const provider = await api(page, 'createModelProvider', {
    expected_revision: settings.models_revision,
    input: { display_name: 'R10 fixture', source: { type: 'custom', catalog_provider_id: null } },
  });
  const endpoint = await api(page, 'createModelEndpoint', {
    expected_revision: provider.revision,
    provider_id: provider.value.id,
    display_name: 'Loopback SSE',
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
      total_ms: 30_000,
    },
    max_parallel_model_requests: 2,
  });
  const profile = await api(page, 'createModelProfile', {
    expected_revision: endpoint.revision,
    input: {
      endpoint_id: endpoint.value.id,
      provider_model_name: 'fixture',
      display_name: 'R10 test model',
      notes: null,
      context_window_tokens: 256000,
    },
  });
  await api(page, 'updateModelProfile', {
    expected_revision: profile.revision,
    model_profile_id: profile.value.id,
    display_name: 'R10 test model',
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
}

export async function fileReadFacts(h, session, turn) {
  const ids = turn.assistant_messages.flatMap((m) => m.tool_call_ids);
  const result = await api(h.page, 'readAgentToolActivity', {
    session_id: session,
    before_call_id: null,
    call_ids: ids,
    limit: 100,
  });
  const read = result.calls.find(
    (c) => c.display.title === '读取文件' && c.display.target === 'note.txt',
  );
  assert.ok(read, 'actual fs.read call exists');
  assert.equal(read.summary.outcome?.type, 'succeeded');
  const body = await api(h.page, 'readAgentResult', {
    session_id: session,
    tool_call_id: read.summary.tool_call_id,
    offset: 0,
    limit: 16_384,
  });
  assert.ok(
    JSON.stringify(body).includes(marker),
    'persisted tool result contains real file content',
  );
  const producing = turn.assistant_messages.findIndex(
    (m) => m.model_step_id === read.model_step_id,
  );
  assert.ok(
    producing >= 0 && producing < turn.assistant_messages.length - 1,
    'tool result followed by another real model step',
  );
  h.record('fs.read success and later model step', {
    callId: read.summary.tool_call_id,
    stepId: read.model_step_id,
    markerVerified: true,
  });
  return result;
}

export async function observeIncremental(page, session) {
  return page.evaluate((sessionId) => {
    const root = document.querySelector(`.agent-conversation[data-session-id="${sessionId}"]`);
    if (!root) throw new Error('Target conversation is not open');
    window.__r10VisibleChanges = [];
    const observer = new MutationObserver(() => {
      const text = [...root.querySelectorAll('.agent-process-text,.agent-reasoning')]
        .map((v) => v.textContent)
        .join('|');
      if (text && window.__r10VisibleChanges.at(-1) !== text)
        window.__r10VisibleChanges.push(text.slice(0, 400));
    });
    observer.observe(root, { childList: true, characterData: true, subtree: true });
  }, session);
}
