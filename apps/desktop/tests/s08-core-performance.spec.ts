import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, readdir, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { performance } from 'node:perf_hooks';
import { armReadOnlyWrite } from './s06/setup';
import { answer, changeMode, finished, launch, submit } from './s06/harness';
import { networkProvider } from './s07/provider';
import { releasePerformanceBundle } from './s08/release-bundle';

/** Model output completion through the committed return received by the local Provider. */
async function latestTiming(root: string, session: string, tool: string, returned: number[]) {
  const matches = (await readdir(root, { recursive: true })).filter((file) =>
    file.endsWith('aura-v2.db'),
  );
  expect(matches).toHaveLength(1);
  const db = new DatabaseSync(join(root, matches[0]!), { readOnly: true });
  try {
    const row = db
      .prepare(
        `SELECT c.id, c.operation_id, b.payload AS batch, c.payload AS call
      FROM tool_calls c JOIN tool_batches b ON b.id=c.batch_id JOIN operations o ON o.id=c.operation_id
      WHERE b.session_id=? AND json_extract(o.payload,'$.content.action_kind')=?
      ORDER BY b.rowid DESC LIMIT 1`,
      )
      .get(session, tool)!;
    expect(row).toBeTruthy();
    const batch = JSON.parse(String(row.batch)) as {
      admitted_at: string;
      batch: { invocation_identity: { model_step_id: string } };
    };
    const step = db
      .prepare('SELECT payload FROM model_steps WHERE id=?')
      .get(batch.batch.invocation_identity.model_step_id)!;
    const startedAt = Date.parse(
      (JSON.parse(String(step.payload)) as { finished_at: string }).finished_at,
    );
    expect(startedAt).toBeLessThanOrEqual(Date.parse(batch.admitted_at));
    const call = JSON.parse(String(row.call)) as {
      result: { outcome: { type: string }; completed_at: string };
    };
    expect(call.result.outcome.type).toBe('succeeded');
    const attempts = db
      .prepare('SELECT COUNT(*) AS n FROM execution_attempts WHERE operation_id=?')
      .get(String(row.operation_id))!;
    expect(Number(attempts.n)).toBe(1);
    // The scripted model sees each turn's one committed return in the request after the call.
    const resultAt = returned.at(-1);
    expect(
      resultAt,
      'the next real Provider request contains the committed tool return',
    ).toBeDefined();
    const elapsed = resultAt! - startedAt;
    const recordedResultMs = Date.parse(call.result.completed_at) - startedAt;
    expect(elapsed).toBeGreaterThanOrEqual(0);
    expect(elapsed).toBeGreaterThanOrEqual(recordedResultMs);
    return {
      call: String(row.id),
      operation: String(row.operation_id),
      elapsed_ms: elapsed,
      recorded_result_ms: recordedResultMs,
      admission_ms: Date.parse(batch.admitted_at) - startedAt,
    };
  } finally {
    db.close();
  }
}

for (const tool of ['shell.powershell', 'shell.cmd']) {
  // eslint-disable-next-line no-empty-pattern -- each shell starts a fresh real Core process and session.
  test(`S08 Release Core ${process.env.AURA_S08_PERF_GIT_TEST_CORE ? '(Git test isolation) ' : ''}${tool} cold and first-command-after-ten-seconds plus warm return times`, async ({}, info) => {
    test.setTimeout(600_000);
    const root = await mkdtemp(join(tmpdir(), 'aura-s08-core-time-'));
    const workspace = join(root, 'workspace');
    await mkdir(workspace);
    // When each turn's committed Shell return reached the model. A Shell result is text and
    // carries no call id, so the times follow the turns.
    const returned: number[] = [];
    const provider = await networkProvider((request) => {
      const received = Date.now();
      const last = request.messages
        .filter((message) => (message as { role?: string }).role === 'tool')
        .at(-1) as { content?: string } | undefined;
      const result = last?.content
        ? (JSON.parse(last.content) as { outcome: { type: string } })
        : undefined;
      if (result?.outcome.type === 'succeeded') returned.push(received);
    });
    const args = {
      script: 'echo core-performance',
      requires_network: false,
      working_directory: '.',
      stdin: '',
      timeout_ms: 10000,
      max_output_bytes: 4096,
    };
    provider.setAction(tool, args);
    const bundle = await releasePerformanceBundle(root);
    let current = await launch(root, {}, bundle.desktop);
    const observations = [];
    try {
      const setup = await armReadOnlyWrite(current.page, workspace, provider.url, {
        title: 'S08 Core performance',
        text: '准备测试会话',
      });
      expect(setup.error).toBeUndefined();
      const session = setup.sessionId as string;
      await answer(current.page, session, 'allow_once');
      await finished(current.page, session);
      await changeMode(current.page, session, 'workspace_read_write');
      await current.app.close();
      provider.setAction(tool, args);
      const started = performance.now();
      current = await launch(root, {}, bundle.desktop);
      const uiReadyMs = performance.now() - started;
      await submit(current.page, session, `${tool} Core 重启后立即调用`);
      await finished(current.page, session);
      const cold = await latestTiming(root, session, tool, returned);
      // The background whole-package check begins during Core composition.
      await new Promise((resolve) =>
        setTimeout(resolve, Math.max(0, 10000 - (performance.now() - started))),
      );
      const samples = [];
      for (let i = 0; i < 20; i += 1) {
        await submit(current.page, session, `${tool} 已准备后的真实命令 ${i}`);
        await finished(current.page, session);
        const sample = await latestTiming(root, session, tool, returned);
        const actual = await current.page.evaluate(
          ({ session_id, tool_call_id }) =>
            window.aura.v1.readShellExecution({
              session_id,
              input: { tool_call_id, stdout_offset: 0, stderr_offset: 0, max_bytes: 4096 },
            }),
          { session_id: session, tool_call_id: sample.call },
        );
        expect(actual.ok, JSON.stringify(actual)).toBe(true);
        if (!actual.ok) throw new Error('actual Shell result unavailable');
        expect(actual.value.exit_code).toBe(0);
        samples.push(sample);
      }
      const sorted = samples.map((sample) => sample.elapsed_ms).sort((a, b) => a - b);
      const p50 = sorted[9]!,
        p95 = sorted[18]!,
        max = sorted[19]!;
      observations.push({
        tool,
        core: bundle,
        ui_ready_ms: uiReadyMs,
        cold_immediate_after_restart: cold,
        first_after_ten_seconds: samples[0],
        samples,
        p50_ms: p50,
        p95_ms: p95,
        max_ms: max,
      });
      const timingPath = info.outputPath(`real-core-timing-${tool}.json`);
      await writeFile(timingPath, JSON.stringify(observations.at(-1), null, 2));
      await info.attach(`real-core-timing-${tool}.json`, {
        path: timingPath,
        contentType: 'application/json',
      });
      expect.soft(p50).toBeLessThanOrEqual(1500);
      expect.soft(p95).toBeLessThanOrEqual(3000);
      expect(provider.errors).toEqual([]);
      const aggregatePath = info.outputPath('real-core-timing.json');
      await writeFile(aggregatePath, JSON.stringify(observations, null, 2));
      await info.attach('real-core-timing.json', {
        path: aggregatePath,
        contentType: 'application/json',
      });
      await info.attach('production-root', { body: root, contentType: 'text/plain' });
    } finally {
      await current.app.close();
      await provider.close();
    }
  });
}
