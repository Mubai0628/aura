// Real Temp/session 1. Preserve old history; record only IDs/states, not chat contents.
//
// The reference turn must be verifiable: the synthetic message names the exact
// source it asks about, the tool results are read through their versioned
// envelope and paged by the offset the store reports, and the old object must
// stay terminal and inactive afterwards.
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import {
  lineageDatabasePath,
  loadTurns,
  namespaceObjectHash,
  resultValue,
} from './history-pages.mjs';
import { harness, api, history, sendNext, sendAddition, poll } from './r10-session-harness.mjs';

const h = await harness('r10-existing-session', { realData: true, capture: false });
let failure;
try {
  const registry = (await api(h.page, 'getWorkspaceSessions')).payload;
  const workspace = registry.workspaces.find((w) => w.display_name === 'Temp');
  assert.ok(workspace, 'Existing Temp workspace is required');
  const session = registry.sessions.find(
    (s) => s.workspace_id === workspace.workspace_id && s.title === '1',
  );
  assert.ok(session, 'Existing session 1 is required; never recreate or rename it');
  const tree = h.page
    .locator('.workspace-tree')
    .filter({ has: h.page.getByRole('button', { name: '在 Temp 新建会话', exact: true }) });
  const expand = tree.getByRole('button', { name: '展开 Temp', exact: true });
  if (await expand.count()) await expand.click();
  await tree.getByRole('button', { name: '1', exact: true }).click();
  await h.page.locator(`.agent-conversation[data-session-id="${session.session_id}"]`).waitFor();
  const model = await poll(
    () => h.page.locator('.model-select').innerText(),
    (value) => /DeepSeek.*Flash/iu.test(value),
    30_000,
    'existing session model projection',
  );
  assert.match(model, /DeepSeek.*Flash/iu, 'Use the already bound non-Pro Flash model');
  assert.doesNotMatch(model, /Pro/iu);
  const initial = await history(h.page, session.session_id);
  assert.equal(initial.active, null, 'Existing session must be idle');
  const knownPage = await loadTurns(api, h.page, session.session_id);
  const known = knownPage.turns;
  // The script must ask about one definite source: the most recent turn that is
  // really interrupted, chosen by its own timestamp instead of page order.
  const sources = known
    .filter((t) => t.turn.state.state === 'recovery_required')
    .sort((a, b) => a.turn.updated_at.localeCompare(b.turn.updated_at));
  const source = sources.at(-1);
  assert.ok(source, 'An existing interrupted turn is required as readable background');
  const sourceCalls = source.assistant_messages.flatMap((message) => message.tool_call_ids);
  assert.equal(
    await h.page.getByRole('button', { name: '核对历史并继续任务', exact: true }).count(),
    0,
    'Explicit continue entry is removed',
  );
  let previous = initial.turns.at(-1).turn.turn_task_id;
  async function newCompleted() {
    const result = await poll(
      () => history(h.page, session.session_id),
      (value) => {
        const turn = value.turns.at(-1)?.turn;
        if (!turn || turn.turn_task_id === previous) return false;
        if (['failed', 'recovery_required', 'cancelled'].includes(turn.state.state))
          throw new Error(`New turn failed: ${turn.state.state}`);
        return turn.state.state === 'completed';
      },
      240_000,
      'existing session completed a new turn',
    );
    const turn = result.turns.at(-1).turn;
    previous = turn.turn_task_id;
    assert.equal(result.inbox.paused, false);
    assert.equal(result.inbox.messages.length, 0);
    await h.page
      .locator(`[data-turn-id="${turn.turn_task_id}"] .agent-turn-status[data-state="completed"]`)
      .waitFor();
    return turn.turn_task_id;
  }
  await sendNext(h, '请只用一句中文问候我，不需要读写文件或运行命令。');
  const ordinary = await newCompleted();
  // This batch: the final answer is rendered exactly once, from the source the
  // projection names. Whether this turn takes the natural or the explicit route
  // is recorded as it really happened.
  const greetingTurns = (await loadTurns(api, h.page, session.session_id)).turns;
  const greeting = greetingTurns.find((item) => item.turn.turn_task_id === ordinary);
  assert.ok(greeting, 'the greeting turn stays visible in the old session');
  const finalSource = greeting.final_text_source;
  assert.ok(finalSource, 'the completed greeting names its final text source');
  assert.ok(['assistant_step', 'completion_report'].includes(finalSource.kind));
  const greetingScope = h.page.locator(`[data-turn-id="${ordinary}"]`);
  assert.equal(await greetingScope.locator('.agent-final-report').count(), 1);
  const finalText = (await greetingScope.locator('.agent-final-report').innerText()).trim();
  assert.ok(finalText.length > 0, 'the real answer is visible');
  await greetingScope.locator('.agent-process-toggle').click();
  const traceTexts = await greetingScope.locator('.agent-process-text').allInnerTexts();
  if (finalSource.kind === 'assistant_step') {
    await greetingScope
      .locator(`.agent-process-step[data-step-id="${finalSource.model_step_id}"]`)
      .waitFor();
    assert.equal(
      await greetingScope
        .locator(
          `.agent-process-step[data-step-id="${finalSource.model_step_id}"] .agent-process-text`,
        )
        .count(),
      0,
      'the final answer is not repeated in the process trace',
    );
  }
  const greetingIds = greeting.assistant_messages.flatMap((message) => message.tool_call_ids);
  const greetingActivity = greetingIds.length
    ? await api(h.page, 'readAgentToolActivity', {
        session_id: session.session_id,
        before_call_id: null,
        call_ids: greetingIds,
        limit: 100,
      })
    : { calls: [] };
  await greetingScope.screenshot({
    path: join(h.output, 'existing-session-final-answer.png'),
  });
  h.record('existing session final answer source and single rendering', {
    turnId: ordinary,
    finalTextSource: finalSource.kind,
    answeredStep: finalSource.kind === 'assistant_step' ? finalSource.model_step_id : null,
    explicitCompletionCalls: greetingActivity.calls.filter(
      (call) => call.summary.display_name === 'normal.complete',
    ).length,
    visibleToolCalls: greetingActivity.calls.map((call) => call.summary.display_name),
    // A reminder round trip shows up as a second real ModelStep for one greeting.
    assistantSteps: greeting.assistant_messages.length,
    visibleTextCharacters: greeting.assistant_messages.map((message) => message.text.length),
    reportCharacters: greeting.turn.state.final_report?.length ?? null,
    traceTexts: traceTexts.length,
    duplicatedInTrace: traceTexts.some((text) => text.trim() === finalText),
    screenshot: 'existing-session-final-answer.png',
  });
  const additionText = '补充：搜索结束后只用一句话说明是否找到，不要再调用工具。';
  await sendNext(
    h,
    '请在当前工作区搜索字面量 AURA-R10-ADDITION-PROBE，只报告是否找到。不要写入、修改或删除任何文件，也不要运行命令。',
  );
  await sendAddition(h, additionText);
  const added = await newCompleted();
  const addedView = (await loadTurns(api, h.page, session.session_id)).turns.find(
    (item) => item.turn.turn_task_id === added,
  );
  assert.ok(addedView, 'the addition stays on the turn that was already running');
  assert.equal(addedView.turn.mode, 'normal');
  assert.ok(
    addedView.user_messages.some((message) => message.text.includes(additionText)),
    'the consumed addition is visible on that turn',
  );
  const addedInbox = await history(h.page, session.session_id);
  assert.equal(addedInbox.inbox.messages.length, 0, 'the addition was not left as a next turn');
  h.record('existing session safety-point addition', {
    turnId: added,
    mode: addedView.turn.mode,
    finalTextSource: addedView.final_text_source?.kind ?? null,
    userMessages: addedView.user_messages.length,
    sameTurn: true,
  });
  await sendNext(
    h,
    `请用任务血缘工具读取这条中断任务的原始来源：turn_task_id=${source.turn.turn_task_id}、lineage_id=${source.turn.lineage_id}。` +
      `然后调用 task_lineage.reconcile，用 reference_prior 记录对 lineage_id=${source.turn.lineage_id} 的引用，` +
      'reason 写明这是对旧来源的只读引用。不要把旧任务改回进行中，不要重放未知写入，也不要读写文件或运行命令。',
  );
  const referenced = await newCompleted();
  const reference = await structuredReference(h.page, session.session_id, referenced, source);
  assert.equal(reference.readSource, true, 'old source was read through the lineage tool');
  assert.equal(reference.referencedPrior, true, 'a structured reference_prior relation was stored');
  assert.equal(reference.oldLineageActive, false, 'the old lineage was not reactivated');
  assert.ok(
    reference.relationCount + reference.derivedCount > 0,
    'the reconcile result records the reference it applied',
  );
  await sendNext(h, '谢谢，请只回复“收到”，不需要任何文件或命令操作。');
  const followup = await newCompleted();
  const finalPage = await loadTurns(api, h.page, session.session_id);
  const finalTurns = finalPage.turns;
  const old = finalTurns.find((t) => t.turn.turn_task_id === source.turn.turn_task_id);
  assert.deepEqual(old?.turn.state, source.turn.state, 'source terminal state remains immutable');
  assert.equal(old?.turn.revision, source.turn.revision);
  const priorToolCallIdsUnchanged =
    JSON.stringify(old?.assistant_messages.flatMap((message) => message.tool_call_ids)) ===
    JSON.stringify(sourceCalls);
  assert.equal(
    priorToolCallIdsUnchanged,
    true,
    'the interrupted turn keeps the same tool-call ids; this does not prove Unknown replay',
  );
  const oldLineage = readLineage(source.turn.lineage_id);
  assert.equal(oldLineage.lineage_id, source.turn.lineage_id);
  assert.notEqual(oldLineage.state, 'active', 'the old lineage stays inactive');
  assert.equal(oldLineage.last_active_turn_task_id, source.turn.turn_task_id);
  const later = finalTurns.filter(
    (t) => t.turn.turn_task_id === referenced || t.turn.turn_task_id === followup,
  );
  assert.equal(later.length, 2, 'the reference turn and the later send are both visible');
  for (const item of later) {
    assert.notEqual(item.turn.lineage_id, source.turn.lineage_id);
    assert.equal(item.turn.lineage_resolution_required, false);
  }
  h.record('existing session ordinary send, structured reference, and follow-up', {
    sessionId: session.session_id,
    source: source.turn.turn_task_id,
    sourceLineage: source.turn.lineage_id,
    sourceState: source.turn.state.state,
    sourceRevision: source.turn.revision,
    requestedTarget: source.turn.lineage_id,
    ordinary,
    referenced,
    followup,
    sourceUnchanged: true,
    priorToolCallIdsUnchanged,
    oldLineage,
    continueEntryAbsent: true,
    readSource: reference.readSource,
    referencedPrior: reference.referencedPrior,
    relationCount: reference.relationCount,
    derivedCount: reference.derivedCount,
    sourceEventMatched: reference.sourceEventMatched,
    pages: {
      turnPages: finalPage.turnPages,
      messagePages: finalPage.messagePages,
      followedCursor: finalPage.turnPages > 1 || finalPage.messagePages > 0,
      exhausted: finalPage.exhausted,
    },
  });
} catch (error) {
  failure = error;
}
await h.finish(failure);

function structuralFacts(value) {
  const eventIds = [];
  const relations = [];
  const derived = [];
  const lineageStates = [];
  const walk = (node) => {
    if (!node || typeof node !== 'object') return;
    if (Array.isArray(node)) {
      node.forEach(walk);
      return;
    }
    if (typeof node.event_id === 'string') eventIds.push(node.event_id);
    if (typeof node.user_event_id === 'string') eventIds.push(node.user_event_id);
    if (typeof node.derived_from_lineage_id === 'string')
      derived.push(node.derived_from_lineage_id);
    if (typeof node.lineage_id === 'string')
      lineageStates.push({
        lineageId: node.lineage_id,
        state: node.state?.state ?? (typeof node.state === 'string' ? node.state : null),
      });
    if (typeof node.earlier_user_event_id === 'string')
      relations.push({ earlier: node.earlier_user_event_id, kind: node.kind ?? null });
    Object.values(node).forEach(walk);
  };
  walk(value);
  return { eventIds, relations, derived, lineageStates };
}

async function structuredReference(page, sessionId, turnId, source) {
  const turns = (await loadTurns(api, page, sessionId)).turns;
  const turn = turns.find((item) => item.turn.turn_task_id === turnId);
  assert.ok(turn, 'referenced turn remains visible');
  const ids = turn.assistant_messages.flatMap((message) => message.tool_call_ids);
  const activity = await api(page, 'readAgentToolActivity', {
    session_id: sessionId,
    before_call_id: null,
    call_ids: ids,
    limit: 100,
  });
  const succeeded = (title) =>
    activity.calls.filter(
      (call) => call.summary.display_name === title && call.summary.outcome?.type === 'succeeded',
    );
  assert.ok(succeeded('task_lineage.read_sources').length > 0, 'the source read succeeded');
  assert.ok(succeeded('task_lineage.reconcile').length > 0, 'the reference reconcile succeeded');
  const sourceEvents = new Set(source.user_messages.map((message) => message.event_id));
  let sourceEventMatched = false;
  for (const call of succeeded('task_lineage.read_sources')) {
    const { value } = await resultValue(api, page, sessionId, call.summary.tool_call_id);
    if (structuralFacts(value).eventIds.some((id) => sourceEvents.has(id))) {
      sourceEventMatched = true;
    }
  }
  let referencedPrior = false;
  let relationCount = 0;
  let derivedCount = 0;
  let oldLineageActive = false;
  for (const call of succeeded('task_lineage.reconcile')) {
    const { value } = await resultValue(api, page, sessionId, call.summary.tool_call_id);
    const facts = structuralFacts(value);
    relationCount += facts.relations.length;
    derivedCount += facts.derived.length;
    if (facts.derived.includes(source.turn.lineage_id)) referencedPrior = true;
    if (facts.relations.some((relation) => sourceEvents.has(relation.earlier)))
      referencedPrior = true;
    if (
      facts.lineageStates.some(
        (entry) => entry.lineageId === source.turn.lineage_id && entry.state === 'active',
      )
    ) {
      oldLineageActive = true;
    }
  }
  return {
    readSource: sourceEventMatched,
    referencedPrior,
    oldLineageActive,
    relationCount,
    derivedCount,
    sourceEventMatched,
  };
}

/** Reads the latest lineage fact from this startup's namespace database. */
function readLineage(lineageId) {
  const manifestPath = join(
    import.meta.dirname,
    '../../../target/aura-dev-runtime/component-manifest.json',
  );
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  if (typeof manifest.bundle_namespace !== 'string' || manifest.bundle_namespace.length === 0) {
    throw new Error('this startup has no bundle namespace');
  }
  const file = lineageDatabasePath(
    process.env.LOCALAPPDATA ?? '',
    namespaceObjectHash(manifest.bundle_namespace),
  );
  if (!existsSync(file)) throw new Error(`this startup has no v2 database at ${file}`);
  const db = new DatabaseSync(file, { readOnly: true });
  try {
    const row = db
      .prepare(
        "SELECT payload FROM events WHERE owner = ? AND kind = 'agent.lineage_state' AND version = 1 ORDER BY sequence DESC LIMIT 1",
      )
      .get(lineageId);
    if (!row?.payload) throw new Error(`authoritative lineage ${lineageId} was not returned`);
    const lineage = JSON.parse(row.payload);
    if (lineage.lineage_id !== lineageId) {
      throw new Error('lineage payload identity did not match the requested id');
    }
    if (typeof lineage.state !== 'string' || lineage.revision == null) {
      throw new Error('lineage state or revision was not returned');
    }
    return {
      lineage_id: lineage.lineage_id,
      state: lineage.state,
      revision: String(lineage.revision),
      last_active_turn_task_id: lineage.last_active_turn_task_id ?? null,
    };
  } finally {
    db.close();
  }
}
