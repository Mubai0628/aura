import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { writeProvider } from './s06-file-write-provider.mjs';
import { armReadOnlyWrite } from './s06/setup';
import {
  answer,
  attemptCount,
  fileGrants,
  finished,
  hash,
  launch,
  pending,
  submit,
} from './s06/harness';

// eslint-disable-next-line no-empty-pattern -- real approval and two production calls.
test('S06 B1 AllowOnce cannot be reused for a new call on the same file', async ({}, info) => {
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s06-once-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const provider = await writeProvider({ path: 'once.txt', text: 'one allowed effect' });
  const { app, page } = await launch(root);
  try {
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: 'S06 Once',
      text: '写入 once.txt',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const workspaceId = setup.workspaceId as string;
    await answer(page, session, 'allow_once');
    await finished(page, session);
    expect(await readFile(join(workspace, 'once.txt'), 'utf8')).toBe('one allowed effect');
    expect(await fileGrants(page, session)).toEqual([]);
    provider.setAction('fs.write', {
      path: 'once.txt',
      text: 'must ask again',
      expected: {
        expected_kind: 'file_hash',
        hash: await hash(page, workspaceId, session, 'once.txt'),
      },
    });
    await submit(page, session, '再次写入 once.txt');
    const next = await pending(page, session);
    expect(await attemptCount(root, next.operation.operation_id)).toBe(0);
    expect(await readFile(join(workspace, 'once.txt'), 'utf8')).toBe('one allowed effect');
    await answer(page, session, 'deny');
    await finished(page, session);
    expect(provider.errors).toEqual([]);
    await info.attach('production-root', { body: root, contentType: 'text/plain' });
  } finally {
    await app.close();
    await provider.close();
  }
});

// eslint-disable-next-line no-empty-pattern -- a second session uses the same native file.
test('S06 B2 Session approval does not transfer to another session', async ({}, info) => {
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s06-other-session-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const provider = await writeProvider({ path: 'private-approval.txt', text: 'first session' });
  const { app, page } = await launch(root);
  try {
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: 'S06 session scope',
      text: '写入 private-approval.txt',
    });
    expect(setup.error).toBeUndefined();
    const first = setup.sessionId as string;
    const workspaceId = setup.workspaceId as string;
    await answer(page, first, 'allow_session');
    await finished(page, first);
    const next = await page.evaluate(
      async ({ workspace_id, model_profile_id }) => {
        const created = await window.aura.v1.createSession({ workspace_id, title: '' });
        if (!created.ok) throw new Error(JSON.stringify(created));
        const session_id = created.value.session_id;
        const before = await window.aura.v1.getSessionPermissionState({ session_id });
        if (!before.ok) throw new Error(JSON.stringify(before));
        const mode = await window.aura.v1.setSessionPermissionMode({
          session_id,
          mode: 'read_only',
          expected_revision: before.value.revision,
        });
        const model = await window.aura.v1.setSessionModelBinding({
          session_id,
          model_profile_id,
          expected_revision: '0',
        });
        if (!mode.ok || !model.ok) throw new Error(JSON.stringify({ mode, model }));
        return session_id;
      },
      { workspace_id: workspaceId, model_profile_id: setup.profileId as string },
    );
    provider.setAction('fs.write', {
      path: 'private-approval.txt',
      text: 'must ask in other session',
      expected: {
        expected_kind: 'file_hash',
        hash: await hash(page, workspaceId, next, 'private-approval.txt'),
      },
    });
    await submit(page, next, '写入同一文件');
    const waiting = await pending(page, next);
    expect(await attemptCount(root, waiting.operation.operation_id)).toBe(0);
    expect(await fileGrants(page, next)).toEqual([]);
    expect((await fileGrants(page, first))[0]!.availability).toBe('in_force');
    expect(await readFile(join(workspace, 'private-approval.txt'), 'utf8')).toBe('first session');
    await answer(page, next, 'deny');
    await finished(page, next);
    expect(provider.errors).toEqual([]);
    await info.attach('production-root', { body: root, contentType: 'text/plain' });
  } finally {
    await app.close();
    await provider.close();
  }
});
