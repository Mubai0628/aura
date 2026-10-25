// Production Main/Preload and authenticated Core IPC; only the model endpoint is replaced.
import { createRequire } from 'node:module';
import { readFile, mkdir, realpath } from 'node:fs/promises';
import { join, resolve, relative, isAbsolute } from 'node:path';
import { setup } from './setup.mjs';
import { history, results } from './metrics.mjs';
import { answerApproval } from './approvals.mjs';

const config = JSON.parse(await readFile(process.argv[2], 'utf8'));
await mkdir(config.data_root, { recursive: true });
const workspace = await realpath(config.workspace);
const aura = await realpath(resolve(config.desktop_root, '../..'));
const rel = relative(aura, workspace);
if (!(rel.startsWith('..') || isAbsolute(rel)))
  throw new Error('eval.workspace_inside_aura_refused');
if (!/^http:\/\/127\.0\.0\.1:\d+\/v1$/.test(config.provider_url))
  throw new Error('eval.unmetered_endpoint_refused');
const require = createRequire(join(config.desktop_root, 'package.json'));
const { _electron: electron } = require('@playwright/test');
const app = await electron.launch({
  args: [`--user-data-dir=${join(config.data_root, 'Electron')}`, config.desktop_root],
  env: {
    ...process.env,
    APPDATA: join(config.data_root, 'Roaming'),
    LOCALAPPDATA: join(config.data_root, 'Local'),
    AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
    AURA_E2E: '1',
    AURA_E2E_CORE_MODE: 'normal',
    AURA_P05_CLEAN_ACCEPTANCE: '1',
    ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
  },
});
let report;
try {
  const page = await app.firstWindow();
  await page.getByText('Aura Core 已就绪', { exact: false }).waitFor({ timeout: 60000 });
  const stats = { models: new Set(), calls: new Set(), unknown: new Set() };
  const started = Date.now();
  const session = await setup(page, config);
  const answered = new Set();
  const approval_answers = [];
  const duration = { model_ms: 0, tools_ms: 0, approval_ms: 0, other_ms: 0 };
  let previous = Date.now();
  let final;
  while (Date.now() - started < config.deadline_seconds * 1000) {
    const view = await history(page, session.session_id, stats);
    const now = Date.now();
    const state = view.active?.run.state.state;
    const bucket =
      state === 'model_running'
        ? 'model_ms'
        : state === 'tools_running'
          ? 'tools_ms'
          : state === 'awaiting_approval'
            ? 'approval_ms'
            : 'other_ms';
    duration[bucket] += now - previous;
    previous = now;
    for (const id of view.active?.pending_approval_ids ?? []) {
      if (answered.has(id)) continue;
      approval_answers.push(
        await answerApproval(page, session.session_id, id, config.approval_decision),
      );
      answered.add(id);
    }
    const turnState = view.active?.turn.state.state ?? view.turns[0]?.turn.state.state;
    if (
      ['completed', 'failed', 'cancelled', 'recovery_required', 'awaiting_query'].includes(
        turnState,
      )
    ) {
      final = view;
      break;
    }
    await new Promise((done) => setTimeout(done, 100));
  }
  let state =
    final?.active?.turn.state.state ?? final?.turns[0]?.turn.state.state ?? 'deadline_stop';
  if (!final || final.active?.run.state.state === 'awaiting_query') {
    const current = final ?? (await history(page, session.session_id, stats));
    if (current.active) {
      const stop = await page.evaluate(
        async (run) =>
          globalThis.window.aura.v1.cancelAgentRun({
            session_id: run.turn.session_id,
            agent_run_id: run.run.agent_run_id,
          }),
        current.active,
      );
      if (!stop.ok) state = 'stop_unconfirmed';
    }
  }
  // Final pagination also covers calls that completed between polling observations.
  await history(page, session.session_id, stats);
  const detail = await results(page, session.session_id, stats);
  report = {
    state,
    approvals: answered.size,
    approval_answers,
    duration_ms: Date.now() - started,
    phase_times: duration,
    phase_timing: '100ms projection polling estimate',
    model_rounds: stats.models.size,
    ...detail,
  };
} finally {
  await app.close();
}
process.stdout.write(JSON.stringify(report));
