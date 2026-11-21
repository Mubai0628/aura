import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { writeProvider } from './s06-file-write-provider.mjs';
import { finished, launch } from './s06/harness';
import { armReadOnlyWrite } from './s06/setup';

// eslint-disable-next-line no-empty-pattern -- This test owns an isolated Electron process.
test('a bad patch hash reaches the model and the visible tool detail before read-only approval', async ({}, info) => {
  test.setTimeout(120_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-tool-input-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const filename = 'notes.txt';
  const original = 'unchanged on disk\n';
  const bad = 'b'.repeat(60);
  await writeFile(join(workspace, filename), original);
  const provider = await writeProvider({
    path: filename,
    text: original,
    auxiliaryJsonOutput: {},
    planAction: ({ step }) => {
      if (step === 0) {
        return {
          tool: 'fs.patch',
          arguments: {
            path: filename,
            expected_hash: { algorithm: 'blake3', hex: bad },
            unified_diff:
              '--- a/notes.txt\n+++ b/notes.txt\n@@ -1 +1 @@\n-unchanged on disk\n+changed\n',
          },
        };
      }
      return step === 1
        ? { tool: 'fs.read', arguments: { path: filename } }
        : { tool: null, arguments: {} };
    },
    finalText: '已重新读取文件。',
  });
  const { app, page } = await launch(root);
  const pageErrors: string[] = [];
  page.on('pageerror', (e) => pageErrors.push(e.message));
  try {
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: '工具参数回归',
      text: '核对修改参数和文件内容。',
    });
    expect(setup.error).toBeUndefined();
    const session = String(setup.sessionId);
    await finished(page, session);
    expect(provider.requests.map((r) => r.step)).toEqual([0, 1, 2]);
    const responses = provider.requests[1]!.messages.filter(
      (m): m is { role: 'tool'; content: string } =>
        !!m &&
        typeof m === 'object' &&
        'role' in m &&
        m.role === 'tool' &&
        'content' in m &&
        typeof m.content === 'string',
    );
    expect(responses).toHaveLength(1);
    const correction = responses[0]!.content;
    for (const required of [
      'tool.schema.arguments_invalid',
      '/expected_hash/hex',
      '64',
      '小写',
      'fs.read',
      'logical_hash',
    ]) {
      expect(correction).toContain(required);
    }
    expect(correction).not.toContain(bad);
    await page.locator('.session-row').first().click();
    const turn = page.locator('.agent-process').last();
    const status = turn.locator('.agent-turn-toggle');
    await expect(status).toBeVisible();
    if ((await status.getAttribute('aria-expanded')) === 'false') await status.click();
    const group = turn.locator('.agent-process-toggle').first();
    await expect(group).toBeVisible();
    if ((await group.getAttribute('aria-expanded')) === 'false') await group.click();
    const failed = turn
      .locator('details.agent-tool-activity')
      .filter({ hasText: '修改文件' })
      .first();
    await expect(failed).toBeVisible();
    if ((await failed.getAttribute('open')) === null)
      await failed.locator('summary').first().click();
    const detail = failed.getByText(/64 个小写十六进制字符/).first();
    await expect(detail).toBeVisible();
    await expect(detail).toContainText('/expected_hash/hex');
    await expect(page.getByRole('region', { name: '当前任务审批' })).toHaveCount(0);
    expect(await readFile(join(workspace, filename), 'utf8')).toBe(original);
    expect(pageErrors).toEqual([]);
    expect(provider.errors).toEqual([]);
    const screenshot = info.outputPath('expanded-correction.png');
    await page.screenshot({ path: screenshot });
    await info.attach('expanded-correction', {
      path: screenshot,
      contentType: 'image/png',
    });
  } finally {
    await app.close();
    await provider.close();
  }
});
