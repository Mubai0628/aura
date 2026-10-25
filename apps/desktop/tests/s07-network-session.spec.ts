import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { answer, attemptCount, changeMode, finished, launch, pending, submit } from './s06/harness';
import { httpService } from './s07/http-service';
import { networkProvider } from './s07/provider';

// eslint-disable-next-line no-empty-pattern -- restart the real Core with the same isolated store.
test('S07 N13 network Session consent survives restart and both mode changes, revoke asks again', async ({}, info) => {
  test.setTimeout(360_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s07-session-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const http = await httpService();
  const provider = await networkProvider();
  provider.setAction('http.request', { url: http.url });
  let current = await launch(root);
  try {
    const setup = await armReadOnlyWrite(current.page, workspace, provider.url, {
      title: 'S07 network session',
      text: '调用本地 API',
    });
    expect(setup.error).toBeUndefined();
    await expect.poll(() => provider.requests.length, { timeout: 30_000 }).toBeGreaterThan(0);
    const session = setup.sessionId as string;
    await answer(current.page, session, 'allow_session');
    expect(JSON.stringify(provider.requests.map((request) => request.tools))).toContain(
      'http_request__',
    );
    await finished(current.page, session);
    expect(http.requests).toHaveLength(1);
    await current.app.close();
    current = await launch(root);
    async function next(label: string) {
      const before = http.requests.length;
      await submit(current.page, session, label);
      await finished(current.page, session);
      expect(http.requests).toHaveLength(before + 1);
    }
    await next('Core 重启后继续');
    await changeMode(current.page, session, 'full_access');
    await next('放宽后继续');
    await changeMode(current.page, session, 'read_only');
    await next('收紧后继续');
    await current.page.getByRole('button', { name: '只读', exact: true }).click();
    await current.page.getByRole('menuitem', { name: '会话授权管理' }).click();
    const dialog = current.page.getByRole('dialog', { name: '会话授权管理' });
    await expect(dialog).toContainText('普通网络工具');
    await dialog.getByRole('button', { name: '撤销联网批准' }).click();
    await expect(dialog).toContainText('已撤销');
    await dialog.getByRole('button', { name: '关闭授权管理' }).click();
    const before = http.requests.length;
    await submit(current.page, session, '撤销后继续');
    const view = await pending(current.page, session);
    expect(await attemptCount(root, view.operation.operation_id)).toBe(0);
    expect(http.requests).toHaveLength(before);
    await answer(current.page, session, 'deny');
    await finished(current.page, session);
    expect(provider.errors).toEqual([]);
    await info.attach('production-root', { body: root, contentType: 'text/plain' });
  } finally {
    await current.app.close();
    await provider.close();
    await http.close();
  }
});
