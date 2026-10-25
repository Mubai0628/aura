/* global window */
// One real non-Pro Flash call chain. Never reads or exports credential values.
import assert from 'node:assert/strict';
import {
  api,
  harness,
  sendNew,
  completed,
  fileReadFacts,
  observeIncremental,
} from './r10-session-harness.mjs';

const h = await harness('r10-deepseek', { realData: true });
let failure;
try {
  const settings = await api(h.page, 'getModelSettings');
  const configured = new Set(
    (settings.active_credentials ?? [])
      .filter((c) => c.state === 'configured')
      .map((c) => c.endpoint_id),
  );
  const candidates = Object.values(settings.registry.models).filter(
    (m) =>
      /^deepseek.*flash/iu.test(m.provider_model_name) &&
      !/pro/iu.test(m.provider_model_name) &&
      m.enabled &&
      configured.has(m.endpoint_id),
  );
  const model = candidates.find((m) => m.provider_model_name === 'deepseek-flash') ?? candidates[0];
  assert.ok(model, 'Configured non-Pro DeepSeek Flash is required; credentials are not modified');
  h.record('real model', {
    name: model.provider_model_name,
    displayName: model.display_name,
    proExcluded: true,
  });
  const session = await sendNew(
    h,
    model.display_name,
    '请先列出当前工作区根目录，再读取 note.txt，用一句中文报告其中的合成校验标记。',
  );
  await observeIncremental(h.page, session);
  const view = await completed(h, session, 1, 240_000);
  const activity = await fileReadFacts(h, session, view.turns.at(-1));
  assert.ok(
    activity.calls.some(
      (c) => c.display.title === '列出目录' && c.summary.outcome?.type === 'succeeded',
    ),
    'actual directory listing succeeded',
  );
  const changes = await h.page.evaluate(() => window.__r10VisibleChanges);
  assert.ok(changes.length >= 2, 'Target conversation must show real incremental output');
  const usage = [];
  for (const message of view.turns.at(-1).assistant_messages) {
    if (!message.context_snapshot_id) continue;
    const inspection = await api(h.page, 'readAgentInspector', {
      session_id: session,
      model_step_id: message.model_step_id,
    });
    usage.push({ step: message.model_step_id, ...inspection.projection.cache_usage });
  }
  const reported = usage.filter(
    (u) => u.actual_input_tokens != null && u.actual_cached_input_tokens != null,
  );
  const input = reported.reduce((n, u) => n + u.actual_input_tokens, 0);
  const cached = reported.reduce((n, u) => n + u.actual_cached_input_tokens, 0);
  h.record('actual API usage (estimates excluded)', {
    steps: usage,
    input,
    cached,
    cacheHitRatio: input > 0 ? cached / input : null,
    allStepsReported: reported.length === usage.length,
  });
  h.record('real window and automatic completion', {
    sessionId: session,
    turnId: view.turns.at(-1).turn.turn_task_id,
    incrementalChanges: changes.length,
    completed: true,
  });
  await h.page.locator('.agent-process-toggle').click();
} catch (error) {
  failure = error;
}
await h.finish(failure);
