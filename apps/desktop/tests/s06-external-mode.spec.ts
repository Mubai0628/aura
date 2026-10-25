import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { Page } from '@playwright/test';
import { writeProvider } from './s06-file-write-provider.mjs';
import { armReadOnlyWrite } from './s06/setup';
import {
  absent,
  answer,
  attemptCount,
  changeMode,
  finished,
  history,
  launch,
  pending,
  submit,
} from './s06/harness';

async function status(page: Page, session: string) {
  const active = (await history(page, session)).active;
  if (!active) return null;
  const result = await page.evaluate(async (request) => window.aura.v1.getAgentRun(request), {
    session_id: session,
    agent_run_id: active.run.agent_run_id,
  });
  if (!result.ok) throw new Error(JSON.stringify(result.error));
  return result.value;
}

// eslint-disable-next-line no-empty-pattern -- only the remote model is substituted.
test('S06 B6 a real read-approved directory asks before write without an Attempt', async ({}, info) => {
  test.setTimeout(240_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s06-b6-'));
  const workspace = join(root, 'workspace');
  const directory = join(root, 'read-approved');
  await mkdir(workspace);
  await mkdir(directory);
  await writeFile(join(directory, 'visible.txt'), 'ordinary fixture');
  const provider = await writeProvider({ path: 'unused.txt', text: '' });
  provider.setAction('fs.list', { path: directory, limit: 100, cursor: null });
  const { app, page } = await launch(root);
  try {
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: 'S06 B6',
      text: '读取外部目录',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await pending(page, session);
    const panel = page.locator('section[aria-label="当前任务审批"]');
    await expect(panel).toContainText('目录');
    await panel.getByRole('button', { name: '本会话允许' }).click();
    await finished(page, session);
    expect(JSON.stringify(provider.requests)).toContain('visible.txt');
    const grants = await page.evaluate(
      async (session_id) => window.aura.v1.listExternalGrants({ session_id }),
      session,
    );
    expect(grants.ok, JSON.stringify(grants)).toBe(true);
    if (!grants.ok) throw new Error('directory grant unavailable');
    expect(
      grants.value.some(
        (grant) =>
          grant.target_kind === 'directory' &&
          grant.capabilities.includes('read') &&
          !grant.capabilities.includes('write'),
      ),
    ).toBe(true);
    const target = join(directory, 'must-ask.txt');
    provider.setAction('fs.write', {
      path: target,
      text: 'must wait',
      expected: { expected_kind: 'absent' },
    });
    await submit(page, session, '写入只批准了读取的目录');
    const write = await pending(page, session);
    expect(write.requested_capabilities).toContain('external_file_write');
    expect(await attemptCount(root, write.operation.operation_id)).toBe(0);
    expect(await absent(target)).toBe(true);
    await answer(page, session, 'deny');
    await finished(page, session);
    expect(await absent(target)).toBe(true);
    expect(await readFile(join(directory, 'visible.txt'), 'utf8')).toBe('ordinary fixture');
    // The management action must withdraw the authority for a fresh subsequent call,
    // including the companion directory record created by that same answer.
    const consents = await page.evaluate(async (session_id) => {
      const listed = await window.aura.v1.listFileSessionConsents({
        session_id,
        include_inactive: true,
      });
      if (!listed.ok) throw new Error(JSON.stringify(listed));
      const grant = listed.value.find((value) => value.availability === 'in_force')!;
      const revoked = await window.aura.v1.revokeFileSessionConsent({
        session_id,
        grant_id: grant.grant_id,
        expected_revision: grant.grant_revision,
      });
      if (!revoked.ok) throw new Error(JSON.stringify(revoked));
      return revoked.value;
    }, session);
    expect(consents.availability).toBe('revoked');
    provider.setAction('fs.list', { path: directory, limit: 100, cursor: null });
    await submit(page, session, '目录批准撤销后重新读取');
    const after = await pending(page, session);
    expect(await attemptCount(root, after.operation.operation_id)).toBe(0);
    await answer(page, session, 'deny');
    await finished(page, session);
    expect(provider.errors).toEqual([]);
    await info.attach('production-root', { body: root, contentType: 'text/plain' });
  } finally {
    await app.close();
    await provider.close();
  }
});

// eslint-disable-next-line no-empty-pattern -- real generation, query and mode routes.
test('S06 E3 real model generation and user query survive loosening and tightening', async ({}, info) => {
  test.setTimeout(240_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s06-e3-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const provider = await writeProvider({ path: 'unused.txt', text: '' });
  provider.setAction(null, {});
  const release = provider.holdNext();
  const { app, page } = await launch(root);
  try {
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: 'S06 E3',
      text: '回答问题',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await expect
      .poll(async () => (await status(page, session))?.run.state.state, { timeout: 90_000 })
      .toBe('model_running');
    const generating = (await status(page, session))!;
    for (const mode of ['full_access', 'read_only'] as const) {
      await changeMode(page, session, mode);
      const live = (await status(page, session))!;
      expect(live.run.agent_run_id).toBe(generating.run.agent_run_id);
      expect(live.run.state.state).toBe('model_running');
      expect(live.cancellation_requested).toBe(false);
    }
    release();
    await finished(page, session);
    provider.setAction('query.user_question', {
      questions: [{ id: 'choice', text: '确认继续？', answer_type: { type: 'confirm' } }],
    });
    await submit(page, session, '询问用户');
    await expect
      .poll(async () => (await status(page, session))?.turn.state.state, { timeout: 90_000 })
      .toBe('awaiting_query');
    const waiting = (await status(page, session))!;
    const query = waiting.pending_query!;
    for (const mode of ['full_access', 'read_only'] as const) {
      await changeMode(page, session, mode);
      const live = (await status(page, session))!;
      expect(live.pending_query?.query_id).toBe(query.query_id);
      expect(live.turn.state.state).toBe('awaiting_query');
      expect(live.cancellation_requested).toBe(false);
    }
    const answered = await page.evaluate(
      async (request) => window.aura.v1.answerAgentQuery(request),
      {
        session_id: session,
        agent_run_id: waiting.run.agent_run_id,
        query_id: query.query_id,
        expected_revision: query.revision,
        answers: [{ question_id: 'choice', value: { answer: 'confirm' as const, accepted: true } }],
      },
    );
    expect(answered.ok, JSON.stringify(answered)).toBe(true);
    await finished(page, session);
    expect(provider.errors).toEqual([]);
    await info.attach('production-root', { body: root, contentType: 'text/plain' });
  } finally {
    release();
    await app.close();
    await provider.close();
  }
});
