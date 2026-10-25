import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { attemptCount, finished, launch, pending } from './s06/harness';
import { httpService } from './s07/http-service';
import { networkProvider } from './s07/provider';

for (const tool of ['http.request', 'http.download']) {
  // eslint-disable-next-line no-empty-pattern -- real Electron/Core and isolated services.
  test(`S07 N12 real approval panel dispatches ${tool} only after approval`, async ({}, info) => {
    test.setTimeout(180_000);
    const root = await mkdtemp(join(tmpdir(), 'aura-s07-panel-'));
    const workspace = join(root, 'workspace');
    await mkdir(workspace);
    const http = await httpService();
    const provider = await networkProvider();
    provider.setAction(
      tool,
      tool === 'http.request'
        ? { url: http.url }
        : { url: `${http.url}/download`, path: 'download.exe' },
    );
    const { app, page } = await launch(root);
    try {
      const setup = await armReadOnlyWrite(page, workspace, provider.url, {
        title: 'S07 network approval',
        text: '调用联网工具完成任务',
      });
      expect(setup.error).toBeUndefined();
      await expect.poll(() => provider.requests.length, { timeout: 30_000 }).toBeGreaterThan(0);
      const view = await pending(page, setup.sessionId as string);
      expect(JSON.stringify(provider.requests.map((request) => request.tools))).toContain(
        `${tool.replaceAll('.', '_')}__`,
      );
      expect(provider.errors).toEqual([]);
      expect(http.requests).toEqual([]);
      expect(await attemptCount(root, view.operation.operation_id)).toBe(0);
      const panel = page.locator('section[aria-label="当前任务审批"]');
      await expect(panel).toContainText('联网');
      await panel.getByRole('button', { name: '仅此一次', exact: true }).click();
      await finished(page, setup.sessionId as string);
      expect(http.requests).toHaveLength(1);
      if (tool === 'http.download')
        expect(await readFile(join(workspace, 'download.exe'), 'utf8')).toBe('S07 real download');
      else expect(JSON.stringify(provider.requests)).toContain('s07-service');
      await info.attach('production-root', { body: root, contentType: 'text/plain' });
    } finally {
      await app.close();
      await provider.close();
      await http.close();
    }
  });
}
