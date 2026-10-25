import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { answer, attemptCount, finished, launch, pending, submit } from './s06/harness';
import { httpService } from './s07/http-service';
import { lastToolValue, networkProvider } from './s07/provider';

// eslint-disable-next-line no-empty-pattern -- use the real Renderer, Core and session artifacts.
test('S07 N2 a real Once fetch permits repeated local web.read without another connection', async ({}, info) => {
  test.setTimeout(360_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s07-document-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const http = await httpService();
  const provider = await networkProvider();
  provider.setAction('web.fetch', { url: `${http.url}/page`, mode: 'http_only' });
  const { app, page } = await launch(root);
  try {
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: 'S07 cached document',
      text: '抓取本地网页',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const approval = await pending(page, session);
    expect(await attemptCount(root, approval.operation.operation_id)).toBe(0);
    expect(http.requests).toHaveLength(0);
    await page
      .locator('section[aria-label="当前任务审批"]')
      .getByRole('button', { name: '仅此一次', exact: true })
      .click();
    await finished(page, session);
    const artifact = lastToolValue(provider.requests.at(-1)!).document_artifact_id;
    expect(artifact).toEqual(expect.any(String));
    for (const offset of [0, 10]) {
      provider.setAction('web.read', {
        document_artifact_id: artifact,
        offset_chars: offset,
        max_chars: 16,
      });
      await submit(page, session, `读取缓存网页 ${offset}`);
      await finished(page, session);
      expect(lastToolValue(provider.requests.at(-1)!).document_artifact_id).toBe(artifact);
      expect(http.requests).toHaveLength(1);
    }
    provider.setAction('web.fetch', { url: `${http.url}/page`, mode: 'http_only' });
    await submit(page, session, '再次抓取需要重新询问');
    const next = await pending(page, session);
    expect(await attemptCount(root, next.operation.operation_id)).toBe(0);
    expect(http.requests).toHaveLength(1);
    await answer(page, session, 'deny');
    await finished(page, session);
    expect(provider.errors).toEqual([]);
    await info.attach('production-root', { body: root, contentType: 'text/plain' });
  } finally {
    await app.close();
    await provider.close();
    await http.close();
  }
});
