import { expect, test } from '@playwright/test';
import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { launch, pending, answer, finished, submit, changeMode } from './s06/harness';
import { armReadOnlyWrite } from './s06/setup';
import { networkProvider } from './s07/provider';

// eslint-disable-next-line no-empty-pattern -- Electron supplies the page; no Playwright browser is launched.
test('Git commits and branches preserve their approval matrix and Native notice through real desktop IPC', async ({}, info) => {
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-git-test-writes-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const global = join(root, 'global.cfg'),
    system = join(root, 'system.cfg'),
    profile = join(root, 'profile.json');
  await writeFile(
    global,
    '[credential]\nhelper =\n[user]\nname = Fixture\nemail = fixture@example.invalid\n[commit]\ngpgsign = false\n',
  );
  await writeFile(system, '');
  const image = process.env.AURA_GIT_TEST_BINARY ?? 'C:/Program Files/Git/cmd/git.exe';
  await writeFile(
    profile,
    JSON.stringify({ global_config: global, system_config: system, git_path: image }),
  );
  const environment = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith('GIT_')),
  );
  const git = (args: string[]) =>
    promisify(execFile)(image, args, {
      cwd: workspace,
      windowsHide: true,
      env: {
        ...environment,
        HOME: root,
        XDG_CONFIG_HOME: root,
        GIT_CONFIG_GLOBAL: global,
        GIT_CONFIG_SYSTEM: system,
        GIT_CONFIG_NOSYSTEM: '1',
        GIT_TERMINAL_PROMPT: '0',
      },
    });
  await git(['init', '--initial-branch=main']);
  await writeFile(join(workspace, 'selected.txt'), 'base\n');
  await writeFile(join(workspace, 'unrelated.txt'), 'base\n');
  await git(['add', '--', 'selected.txt', 'unrelated.txt']);
  await git(['commit', '-m', 'fixture base']);
  await writeFile(join(workspace, 'selected.txt'), 'selected change\n');
  await writeFile(join(workspace, 'unrelated.txt'), 'keep staged\n');
  await git(['add', '--', 'unrelated.txt']);
  const before = (await git(['rev-parse', 'HEAD'])).stdout.trim();
  const provider = await networkProvider();
  provider.setAction('git.commit', { message: 'desktop selected commit', paths: ['selected.txt'] });
  const { app, page } = await launch(root, { AURA_GIT_TEST_PROFILE: profile });
  try {
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: 'L03 Native Git',
      text: '只提交 selected.txt，并写说明 desktop selected commit。',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const proposal = await pending(page, session);
    expect(proposal.backend.class).toBe('native');
    expect(proposal.operation.content.capabilities).toEqual(['git_write']);
    expect(proposal.backend.guarantees.slice().sort()).toEqual([
      'bounded_output',
      'child_process_control',
    ]);
    // The card shows the model's explanation; the facts are under 查看详情.
    await page.locator('.agent-approval').getByText('查看详情').click();
    await expect(
      page
        .locator('.agent-approval')
        .getByText('与在终端运行 git 相同，Aura 不隔离它的文件写入', { exact: true }),
    ).toBeVisible();
    expect((await git(['rev-parse', 'HEAD'])).stdout.trim()).toBe(before);
    await answer(page, session, 'allow_once');
    await finished(page, session);
    expect((await git(['show', 'HEAD:selected.txt'])).stdout).toBe('selected change\n');
    expect((await git(['show', 'HEAD:unrelated.txt'])).stdout).toBe('base\n');
    expect((await git(['diff', '--cached', '--name-only'])).stdout).toBe('unrelated.txt\n');
    await changeMode(page, session, 'workspace_read_write');
    provider.setAction('git.branch', { kind: 'create', name: 'l03-desktop', switch: true });
    await submit(page, session, '新建并切换到 l03-desktop。');
    await finished(page, session);
    expect((await git(['symbolic-ref', '--short', 'HEAD'])).stdout).toBe('l03-desktop\n');
    const activity = await page.evaluate(async (session_id) => {
      const value = await window.aura.v1.readAgentToolActivity({
        session_id,
        before_call_id: null,
        limit: 100,
      });
      if (!value.ok) throw new Error(value.error.message);
      return value.value.calls;
    }, session);
    for (const name of ['git.commit', 'git.branch']) {
      const call = activity.find((value) => value.summary.display_name === name);
      expect(call, name).toBeDefined();
      expect(call!.summary.outcome?.type).toBe('succeeded');
      expect(call!.display.summary).toContain('与在终端运行 git 相同，Aura 不隔离它的文件写入');
    }
    expect(provider.errors).toEqual([]);
    await info.attach('temporary-root', { body: root, contentType: 'text/plain' });
  } finally {
    await app.close();
    await provider.close();
  }
});
