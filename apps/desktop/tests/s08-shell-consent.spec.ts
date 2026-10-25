import { expect, test, type Page } from '@playwright/test';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { answer, attemptCount, changeMode, finished, launch, pending, submit } from './s06/harness';
import { networkProvider } from './s07/provider';

async function consents(page: Page, session: string) {
  return page.evaluate(async (session_id) => {
    const run = await window.aura.v1.listNativeSessionConsents({
      session_id,
      include_inactive: true,
    });
    const network = await window.aura.v1.listNetworkSessionConsents({
      session_id,
      include_inactive: true,
    });
    if (!run.ok || !network.ok) throw new Error(JSON.stringify({ run, network }));
    return { run: run.value, network: network.value };
  }, session);
}
const command = (script: string, network = false) => ({
  script,
  requires_network: network,
  working_directory: '.',
  stdin: '',
  timeout_ms: 10000,
  max_output_bytes: 4096,
});

// eslint-disable-next-line no-empty-pattern -- actual Electron and Core restarts use isolated data.
test('S08 run and network Session answers survive Core restart, both mode directions and independent revocation', async ({}, info) => {
  test.setTimeout(420_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s08-session-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  let networkHits = 0;
  const service = createServer((_request, response) => {
    networkHits += 1;
    response.end('S08 local service');
  });
  await new Promise<void>((resolve) => service.listen(0, '127.0.0.1', resolve));
  const address = service.address();
  if (!address || typeof address === 'string') throw new Error('local service address unavailable');
  await writeFile(
    join(workspace, 'network.cjs'),
    `require('http').get('http://127.0.0.1:${address.port}/proof', r => {
      console.log(r.statusCode); r.resume();
    }).on('error', e => { console.error(e.message); process.exitCode = 1; });`,
  );
  const provider = await networkProvider();
  provider.setAction('shell.cmd', command('echo first>shell-effect.txt'));
  let current = await launch(root);
  try {
    const setup = await armReadOnlyWrite(current.page, workspace, provider.url, {
      title: 'S08 shell',
      text: '执行本地 CMD',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const initial = await pending(current.page, session);
    expect(initial.requested_capabilities).toEqual(['process_execute']);
    expect(await attemptCount(root, initial.operation.operation_id)).toBe(0);
    const panel = current.page.locator('section[aria-label="当前任务审批"]');
    await expect(panel).toContainText('不受系统级只读或目录限制');
    await panel.getByRole('button', { name: '本会话允许' }).click();
    await finished(current.page, session);
    expect(await readFile(join(workspace, 'shell-effect.txt'), 'utf8')).toContain('first');
    expect((await consents(current.page, session)).run[0]?.availability).toBe('in_force');
    expect((await consents(current.page, session)).network).toHaveLength(0);
    provider.setAction(
      'shell.cmd',
      command('node network.cjs && echo network>>shell-effect.txt', true),
    );
    await submit(current.page, session, '声明联网的命令');
    const question = await pending(current.page, session);
    expect(question.requested_capabilities).toEqual(['ordinary_network']);
    expect(await attemptCount(root, question.operation.operation_id)).toBe(0);
    await answer(current.page, session, 'allow_session');
    await finished(current.page, session);
    expect(networkHits).toBe(1);
    const before = await consents(current.page, session);
    expect(before.run.filter((value) => value.availability === 'in_force')).toHaveLength(1);
    expect(
      before.network.filter(
        (value) => value.availability === 'in_force' && value.scope.caller === 'built_in_shells',
      ),
    ).toHaveLength(1);
    await current.app.close();
    current = await launch(root);
    async function reuse(label: string) {
      const old = await readFile(join(workspace, 'shell-effect.txt'), 'utf8');
      const previousHits = networkHits;
      await submit(current.page, session, label);
      await finished(current.page, session);
      expect((await readFile(join(workspace, 'shell-effect.txt'), 'utf8')).length).toBeGreaterThan(
        old.length,
      );
      expect(networkHits).toBe(previousHits + 1);
      const after = await consents(current.page, session);
      expect(
        after.run.find((value) => value.grant_id === before.run[0]?.grant_id)?.availability,
      ).toBe('in_force');
      expect(
        after.network.find((value) => value.grant_id === before.network[0]?.grant_id)?.availability,
      ).toBe('in_force');
    }
    await reuse('Core 重启后复用');
    await changeMode(current.page, session, 'full_access');
    await reuse('放宽后复用');
    await changeMode(current.page, session, 'read_only');
    await reuse('收紧后复用');
    await current.page.getByRole('button', { name: '只读', exact: true }).click();
    await current.page.getByRole('menuitem', { name: '会话授权管理' }).click();
    const dialog = current.page.getByRole('dialog', { name: '会话授权管理' });
    await expect(dialog).toContainText('Shell 运行批准');
    await expect(dialog).toContainText('Shell 联网批准');
    await dialog.getByRole('button', { name: '撤销联网批准' }).click();
    await expect(dialog).toContainText('已撤销');
    await dialog.getByRole('button', { name: '关闭授权管理' }).click();
    const hitsBeforeRefusal = networkHits;
    await submit(current.page, session, '撤销联网后再调用');
    const withdrawn = await pending(current.page, session);
    expect(withdrawn.requested_capabilities).toEqual(['ordinary_network']);
    expect(await attemptCount(root, withdrawn.operation.operation_id)).toBe(0);
    await answer(current.page, session, 'deny');
    await finished(current.page, session);
    expect(networkHits).toBe(hitsBeforeRefusal);
    provider.setAction('shell.cmd', command('echo run-reused>>shell-effect.txt'));
    await submit(current.page, session, '仍可使用运行批准');
    await finished(current.page, session);
    const running = (await consents(current.page, session)).run.find(
      (value) => value.availability === 'in_force',
    )!;
    await current.page.getByRole('button', { name: '只读', exact: true }).click();
    await current.page.getByRole('menuitem', { name: '会话授权管理' }).click();
    await dialog.getByRole('button', { name: '撤销 Shell 运行批准' }).click();
    await expect
      .poll(
        async () =>
          (await consents(current.page, session)).run.find(
            (value) => value.grant_id === running.grant_id,
          )?.availability,
      )
      .toBe('revoked');
    await dialog.getByRole('button', { name: '关闭授权管理' }).click();
    await submit(current.page, session, '撤销运行后再调用');
    const runQuestion = await pending(current.page, session);
    expect(runQuestion.requested_capabilities).toEqual(['process_execute']);
    expect(await attemptCount(root, runQuestion.operation.operation_id)).toBe(0);
    await answer(current.page, session, 'deny');
    await finished(current.page, session);
    expect(provider.errors).toEqual([]);
    await info.attach('production-root', { body: root, contentType: 'text/plain' });
  } finally {
    await current.app.close();
    await provider.close();
    await new Promise<void>((resolve, reject) =>
      service.close((error) => (error ? reject(error) : resolve())),
    );
  }
});

// eslint-disable-next-line no-empty-pattern -- same Core and workspace, separate real sessions.
test('S08 denial creates no process or Attempt and another session cannot inherit running approval', async ({}, info) => {
  test.setTimeout(240_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s08-isolation-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const provider = await networkProvider();
  provider.setAction('shell.cmd', command('echo forbidden>forbidden.txt'));
  const current = await launch(root);
  try {
    const setup = await armReadOnlyWrite(current.page, workspace, provider.url, {
      title: 'S08 isolation',
      text: '拒绝运行',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const refused = await pending(current.page, session);
    await answer(current.page, session, 'deny');
    await finished(current.page, session);
    expect(await attemptCount(root, refused.operation.operation_id)).toBe(0);
    await expect(readFile(join(workspace, 'forbidden.txt'))).rejects.toThrow();
    provider.setAction('shell.cmd', command('echo own>own.txt'));
    await submit(current.page, session, '批准本会话');
    await answer(current.page, session, 'allow_session');
    await finished(current.page, session);
    const other = await current.page.evaluate(
      async ({ workspace_id, profile_id }) => {
        const api = window.aura.v1;
        const created = await api.createSession({ workspace_id, title: 'S08 another' });
        if (!created.ok) throw new Error(JSON.stringify(created));
        const session_id = created.value.session_id;
        const before = await api.getSessionPermissionState({ session_id });
        if (!before.ok) throw new Error(JSON.stringify(before));
        const changed = await api.setSessionPermissionMode({
          session_id,
          expected_revision: before.value.revision,
          mode: 'read_only',
        });
        if (!changed.ok) throw new Error(JSON.stringify(changed));
        const binding = await api.setSessionModelBinding({
          session_id,
          expected_revision: '0',
          model_profile_id: profile_id,
        });
        if (!binding.ok) throw new Error(JSON.stringify(binding));
        return session_id;
      },
      { workspace_id: setup.workspaceId as string, profile_id: setup.profileId as string },
    );
    provider.setAction('shell.cmd', command('echo other>other.txt'));
    await submit(current.page, other, '另一个会话');
    const question = await pending(current.page, other);
    expect(await attemptCount(root, question.operation.operation_id)).toBe(0);
    await expect(readFile(join(workspace, 'other.txt'))).rejects.toThrow();
    await answer(current.page, other, 'deny');
    await finished(current.page, other);
    expect(provider.errors).toEqual([]);
    await info.attach('production-root', { body: root, contentType: 'text/plain' });
  } finally {
    await current.app.close();
    await provider.close();
  }
});
