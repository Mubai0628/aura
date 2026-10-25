import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, readFile, rename, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { writeProvider } from './s06-file-write-provider.mjs';
import { armReadOnlyWrite } from './s06/setup';
import {
  absent,
  answer,
  attemptCount,
  changeMode,
  fileGrants,
  finished,
  hash,
  launch,
  pending,
  submit,
} from './s06/harness';

// eslint-disable-next-line no-empty-pattern -- real Electron and production Core are explicit.
test('S06 A1 real ReadOnly reads without approval', async ({}, info) => {
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s06-a1-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  await writeFile(join(workspace, 'read.txt'), 's06-read-marker');
  const provider = await writeProvider({ path: 'read.txt', text: '' });
  provider.setAction('fs.read', {
    path: 'read.txt',
    range: { range_kind: 'full' },
    explicit_encoding: null,
  });
  const { app, page } = await launch(root);
  try {
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: 'S06 A1',
      text: '读取 read.txt',
    });
    expect(setup.error).toBeUndefined();
    await finished(page, setup.sessionId as string);
    expect(JSON.stringify(provider.requests)).toContain('s06-read-marker');
    expect(await fileGrants(page, setup.sessionId as string)).toEqual([]);
    expect(provider.errors).toEqual([]);
    await info.attach('production-root', { body: root, contentType: 'text/plain' });
  } finally {
    await app.close();
    await provider.close();
  }
});

// eslint-disable-next-line no-empty-pattern -- explicit real Core restart on the same isolated store.
test('S06 B2 B4 F1 F2 Session file approval survives new calls, restart and both mode directions', async ({}, info) => {
  test.setTimeout(360_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s06-session-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const target = join(workspace, 'approved.txt');
  const provider = await writeProvider({ path: 'approved.txt', text: 'first' });
  let current = await launch(root);
  try {
    const setup = await armReadOnlyWrite(current.page, workspace, provider.url, {
      title: 'S06 persistent',
      text: '写入 approved.txt',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const workspaceId = setup.workspaceId as string;
    const view = await pending(current.page, session);
    expect(await attemptCount(root, view.operation.operation_id)).toBe(0);
    expect(await absent(target)).toBe(true);
    const panel = current.page.locator('section[aria-label="当前任务审批"]');
    await expect(panel).toBeVisible();
    await expect(panel).toContainText('模式变化和 Core 重启后仍有效');
    await panel.getByRole('button', { name: '本会话允许' }).click();
    await finished(current.page, session);
    expect(await readFile(target, 'utf8')).toBe('first');
    const grant = (await fileGrants(current.page, session))[0]!;
    expect(grant.availability).toBe('in_force');
    expect(grant.scope.targets[0]!.canonical_path).toContain('approved.txt');
    async function next(text: string) {
      const before = await hash(current.page, workspaceId, session, 'approved.txt');
      provider.setAction('fs.write', {
        path: 'approved.txt',
        text,
        expected: { expected_kind: 'file_hash', hash: before },
      });
      await submit(current.page, session, `继续写入 ${text}`);
      await finished(current.page, session);
      expect(await readFile(target, 'utf8')).toBe(text);
      expect(
        (await fileGrants(current.page, session)).filter((g) => g.availability === 'in_force'),
      ).toHaveLength(1);
    }
    await next('second call');
    await current.app.close();
    current = await launch(root);
    expect((await fileGrants(current.page, session))[0]!.availability).toBe('in_force');
    await next('after real Core restart');
    await changeMode(current.page, session, 'full_access');
    await next('after loosening');
    await changeMode(current.page, session, 'read_only');
    await next('after tightening');
    provider.setAction('fs.write', {
      path: 'unapproved.txt',
      text: 'must ask',
      expected: { expected_kind: 'absent' },
    });
    await submit(current.page, session, '写入另一文件');
    const other = await pending(current.page, session);
    expect(await attemptCount(root, other.operation.operation_id)).toBe(0);
    expect(await absent(join(workspace, 'unapproved.txt'))).toBe(true);
    await answer(current.page, session, 'deny');
    await finished(current.page, session);
    // F2: use the real permission menu and the real management dialog.
    await current.page.getByRole('button', { name: '只读', exact: true }).click();
    await current.page.getByRole('menuitem', { name: '会话授权管理' }).click();
    const dialog = current.page.getByRole('dialog', { name: '会话授权管理' });
    await expect(dialog).toContainText('approved.txt');
    await dialog.getByRole('button', { name: '撤销文件批准' }).click();
    await expect
      .poll(async () => (await fileGrants(current.page, session))[0]!.availability)
      .toBe('revoked');
    await dialog.getByRole('button', { name: '关闭授权管理' }).click();
    provider.setAction('fs.write', {
      path: 'approved.txt',
      text: 'after revoke',
      expected: {
        expected_kind: 'file_hash',
        hash: await hash(current.page, workspaceId, session, 'approved.txt'),
      },
    });
    await submit(current.page, session, '撤销后重试');
    const revoked = await pending(current.page, session);
    expect(await attemptCount(root, revoked.operation.operation_id)).toBe(0);
    expect(await readFile(target, 'utf8')).toBe('after tightening');
    await answer(current.page, session, 'allow_session');
    await finished(current.page, session);
    // A different object at the same spelling requires a fresh answer.
    await rename(target, join(workspace, 'moved-original.txt'));
    await writeFile(target, 'different object');
    expect(
      (await fileGrants(current.page, session)).some((g) => g.availability === 'target_changed'),
    ).toBe(true);
    provider.setAction('fs.write', {
      path: 'approved.txt',
      text: 'replace object',
      expected: {
        expected_kind: 'file_hash',
        hash: await hash(current.page, workspaceId, session, 'approved.txt'),
      },
    });
    await submit(current.page, session, '对象替换后重试');
    const replaced = await pending(current.page, session);
    expect(await attemptCount(root, replaced.operation.operation_id)).toBe(0);
    expect(await readFile(target, 'utf8')).toBe('different object');
    await answer(current.page, session, 'deny');
    await finished(current.page, session);
    expect(provider.errors).toEqual([]);
    await info.attach('production-root-and-scope', {
      body: JSON.stringify({ root, session, grant }),
      contentType: 'application/json',
    });
  } finally {
    await current.app.close();
    await provider.close();
  }
});
