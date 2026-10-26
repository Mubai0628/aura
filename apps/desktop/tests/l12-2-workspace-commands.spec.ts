import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { answer, changeMode, finished, history, launch, pending, submit } from './s06/harness';
import { networkProvider } from './s07/provider';

// In workspace read-write commands run without asking; a download asks once per session,
// a command that writes outside the workspace or deletes files asks each time.

const command = (script: string) => ({
  script,
  requires_network: false,
  working_directory: '.',
  stdin: '',
  timeout_ms: 10000,
  max_output_bytes: 4096,
});

// eslint-disable-next-line no-empty-pattern -- one isolated Electron and Core.
test('workspace read-write runs commands and asks for downloads, outside writes and deletions', async ({}, info) => {
  test.setTimeout(420_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l12-2-commands-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  await writeFile(join(workspace, 'keep.txt'), 'keep me\n');
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const provider = await networkProvider();
  provider.setAction('shell.cmd', command('echo read-only>effect.txt'));
  const current = await launch(root);
  const unasked = async (session: string) => {
    await finished(current.page, session);
    expect((await history(current.page, session)).active?.pending_approval_ids ?? []).toHaveLength(
      0,
    );
  };
  try {
    const setup = await armReadOnlyWrite(current.page, workspace, provider.url, {
      title: 'Workspace commands',
      text: '先在只读下运行一次',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    // Read-only asks before running.
    await pending(current.page, session);
    await answer(current.page, session, 'allow_once');
    await finished(current.page, session);

    await changeMode(current.page, session, 'workspace_read_write');
    provider.setAction('shell.cmd', command('echo first>>effect.txt'));
    await submit(current.page, session, '工作区读写下的命令');
    await unasked(session);
    expect(await readFile(join(workspace, 'effect.txt'), 'utf8')).toContain('first');
    provider.setAction('shell.powershell', command('Add-Content -Path effect.txt -Value again'));
    await submit(current.page, session, '同一会话里的 PowerShell');
    await unasked(session);
    expect(await readFile(join(workspace, 'effect.txt'), 'utf8')).toContain('again');

    // A download asks once; the session answer covers the next one. The workspace is not a
    // repository, so neither reaches the network.
    provider.setAction('shell.cmd', command('git fetch'));
    await submit(current.page, session, '下载');
    const download = await pending(current.page, session);
    expect(download.requested_capabilities).toEqual(['download_artifact']);
    const panel = current.page.locator('section[aria-label="当前任务审批"]');
    await panel.getByRole('button', { name: '本会话允许' }).click();
    await finished(current.page, session);
    provider.setAction('shell.cmd', command('git pull'));
    await submit(current.page, session, '再次下载');
    await unasked(session);

    // A write outside the workspace asks, and can be allowed once only.
    const outside = join(root, 'outside.txt');
    provider.setAction('shell.cmd', command(`echo x>"${outside}"`));
    await submit(current.page, session, '写到工作区以外');
    await pending(current.page, session);
    await expect(panel.getByText('这条命令会写工作区以外的位置；只能允许这一次。')).toBeVisible();
    await expect(panel.getByRole('button', { name: '本会话允许' })).toHaveCount(0);
    await answer(current.page, session, 'deny');
    await finished(current.page, session);
    expect(existsSync(outside)).toBe(false);

    provider.setAction('shell.cmd', command('del keep.txt'));
    await submit(current.page, session, '删除文件的命令');
    await pending(current.page, session);
    await answer(current.page, session, 'deny');
    await finished(current.page, session);
    expect(await readFile(join(workspace, 'keep.txt'), 'utf8')).toBe('keep me\n');
  } finally {
    await current.app.close();
    await provider.close();
  }
});
