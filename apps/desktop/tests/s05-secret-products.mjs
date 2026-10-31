// Same authenticated Core as the Tool Secret UI: a real credentialed Model call,
// ordinary Agent tools, durable continuation and six classes of actual output.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { api, completed, fileReadFacts, marker, sendNew } from './r10-session-harness.mjs';

export async function captureSecretProducts({
  page,
  root,
  endpointId,
  output,
  provider,
  modelRequestBodies,
  responses,
}) {
  const settings = await api(page, 'getModelSettings');
  const profile = await api(page, 'createModelProfile', {
    expected_revision: settings.models_revision,
    input: {
      endpoint_id: endpointId,
      provider_model_name: 'fixture',
      display_name: 'S05 Secret proof model',
      notes: null,
      context_window_tokens: 256000,
    },
  });
  responses.push(
    await api(page, 'updateModelProfile', {
      expected_revision: profile.revision,
      model_profile_id: profile.value.id,
      display_name: 'S05 Secret proof model',
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
    }),
  );
  const workspaceRoot = join(root, 'workspace');
  await mkdir(workspaceRoot);
  await writeFile(join(workspaceRoot, 'note.txt'), `Actual S05 file result: ${marker}\n`);
  const name = 'S05 Secret output proof';
  const workspace = await api(page, 'createWorkspace', {
    display_name: name,
    selected_root: workspaceRoot,
  });
  responses.push(workspace);
  await page.getByRole('button', { name: '返回会话', exact: true }).click();
  const measured = [];
  const h = {
    page,
    name,
    workspaceId: workspace.workspace.workspace_id,
    record(check, value) {
      measured.push({ check, value });
    },
  };
  const session = await sendNew(h, 'S05 Secret proof model', '读取目录及文件并报告结果');
  const view = await completed(h, session);
  const turn = view.turns.at(-1);
  const activity = await fileReadFacts(h, session, turn);
  responses.push(view, activity);
  assert.equal(provider.errors.length, 0);
  assert.ok(provider.requests.length >= 2, 'a tool result must reach a later real model request');
  const namespaceNames = await readdir(join(root, 'Aura/namespaces'));
  assert.equal(namespaceNames.length, 1, 'only this fixture owns the data namespace');
  const namespace = join(root, 'Aura/namespaces', namespaceNames[0]);
  const databasePath = join(namespace, 'data/v2-execution/aura-v2.db');
  const db = new DatabaseSync(databasePath, { readOnly: true });
  let events, projections, contexts, artifacts;
  try {
    db.exec('BEGIN'); // one read snapshot of this actual running Core's committed products
    events = db.prepare('SELECT event_id,owner,kind,payload FROM events ORDER BY sequence').all();
    projections = db
      .prepare('SELECT event_id,topic,revision,payload FROM projection_outbox ORDER BY sequence')
      .all();
    contexts = db
      .prepare('SELECT id,payload FROM context_snapshots WHERE session_id=? ORDER BY rowid')
      .all(session);
    artifacts = db
      .prepare(
        'SELECT a.id,a.object_hash,o.bytes FROM artifacts a JOIN artifact_objects o ON o.hash=a.object_hash ORDER BY a.rowid',
      )
      .all();
    db.exec('COMMIT');
  } finally {
    db.close();
  }
  // Core no longer copies events into projection_outbox (store slimming), so that table may be
  // empty; whatever rows it holds are still scanned below.
  for (const [name, rows] of Object.entries({ events, contexts, artifacts }))
    assert.ok(rows.length > 0, `${name}: missing products cannot constitute a clean scan`);
  const artifactBytes = Buffer.concat(
    artifacts.flatMap((row) => [Buffer.from(row.id + '\n'), Buffer.from(row.bytes)]),
  );
  assert.ok(
    artifactBytes.includes(Buffer.from(marker)),
    'actual file bytes must be in an artifact',
  );
  const products = {
    ipc: Buffer.from(JSON.stringify(responses)),
    logs: Buffer.concat([
      await readFile(join(root, 'Aura/logs/core.jsonl')),
      await readFile(join(namespace, 'logs/main.jsonl')),
    ]),
    events: Buffer.from(JSON.stringify(events)),
    projections: Buffer.from(JSON.stringify(projections)),
    context: Buffer.from(JSON.stringify({ stored: contexts, model_requests: modelRequestBodies })),
    artifacts: artifactBytes,
  };
  await mkdir(output); // unique Playwright output; never overwrite a former run
  const facts = [];
  for (const [className, bytes] of Object.entries(products)) {
    assert.ok(bytes.length > 0);
    await writeFile(join(output, className + '.bin'), bytes);
    facts.push({
      class: className,
      file: className + '.bin',
      bytes: bytes.length,
      sha256: createHash('sha256').update(bytes).digest('hex'),
      origin: output,
      captured_from:
        className === 'ipc'
          ? 'ordinary Main/Preload response over authenticated Named Pipe'
          : className === 'logs'
            ? 'this Core and Main diagnostic files'
            : className === 'context'
              ? 'committed Agent context snapshots and actual later Model request bodies'
              : 'this Core BrokerJournal read snapshot',
      source_database: ['events', 'projections', 'context', 'artifacts'].includes(className)
        ? databasePath
        : null,
      session_id: session,
      turn_task_id: turn.turn.turn_task_id,
      tool_call_ids: activity.calls.map((call) => call.summary.tool_call_id),
      context_snapshot_ids: contexts.map((row) => row.id),
      artifact_ids: artifacts.map((row) => row.id),
      event_ids: events.map((row) => row.event_id),
      authenticated_ipc: true,
    });
  }
  await writeFile(join(output, 'facts.json'), JSON.stringify({ facts, measured }, null, 2));
  return products;
}
