import { expect, test } from '@playwright/test';
import { copyFile, mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { launch } from './s06/harness';

test('native Git runtime gate crosses Main, Core and preload and redetects without restarting', async () => {
  test.setTimeout(120_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-git-test-runtime-'));
  const work = join(root, 'workspace');
  await mkdir(work);
  const global = join(root, 'global.cfg'),
    system = join(root, 'system.cfg'),
    profile = join(root, 'profile.json');
  await writeFile(global, '[credential]\nhelper =\n');
  await writeFile(system, '');
  const realGit = process.env.AURA_GIT_TEST_BINARY ?? 'C:/Program Files/Git/cmd/git.exe';
  const setupEnvironment = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith('GIT_')),
  );
  const git = (args: string[]) =>
    promisify(execFile)(realGit, args, {
      cwd: work,
      env: {
        ...setupEnvironment,
        HOME: root,
        XDG_CONFIG_HOME: root,
        GIT_CONFIG_GLOBAL: global,
        GIT_CONFIG_SYSTEM: system,
        GIT_CONFIG_NOSYSTEM: '1',
        GIT_TERMINAL_PROMPT: '0',
      },
    });
  await git(['init', '--initial-branch=main']);
  await writeFile(join(work, 'tracked.txt'), 'before\n');
  await git(['add', 'tracked.txt']);
  await git([
    '-c',
    'user.name=Fixture',
    '-c',
    'user.email=fixture@example.invalid',
    '-c',
    'commit.gpgsign=false',
    'commit',
    '-m',
    'B1 fixture',
  ]);
  await writeFile(join(work, 'tracked.txt'), 'after\n');
  const image = join(root, 'git.exe');
  await copyFile(
    resolve(import.meta.dirname, '../../../target/debug/aura-git-version-fixture.exe'),
    image,
  );
  await writeFile(join(root, 'version.txt'), 'git version 2.39.0.windows.1\n');
  const select = async (git_path: string) =>
    writeFile(profile, JSON.stringify({ global_config: global, system_config: system, git_path }));
  await select(image);
  const { app, page } = await launch(root, { AURA_GIT_TEST_PROFILE: profile });
  const pageErrors: string[] = [];
  page.on('pageerror', (error) => pageErrors.push(error.message));
  try {
    await page.getByRole('button', { name: '设置', exact: true }).click();
    // Git is under 设置 → 环境.
    await page
      .locator('.settings-navigation')
      .getByRole('button', { name: '环境', exact: true })
      .click();
    const panel = page.getByRole('region', { name: 'Git 运行环境' });
    await expect(panel).toContainText('本机 Git 版本低于 2.40');
    await expect(panel.getByRole('button', { name: '前往 Git 官网安装或升级' })).toBeVisible();
    const readCatalog = () =>
      page.evaluate(async () => {
        const result = await window.aura.v1.readAgentToolSettings({});
        if (!result.ok) throw new Error(result.error.message);
        return result.value.tools
          .filter((tool) => tool.name.startsWith('git.'))
          .map((tool) => tool.available);
      });
    expect(await readCatalog()).toEqual(Array<boolean>(7).fill(false));
    await select(join(root, 'missing-git.exe'));
    await panel.getByRole('button', { name: '重新检测' }).click();
    await expect(panel).toContainText('未找到本机 Git');
    // Same live Core and window. Use the existing real installation, never owner config.
    await select(realGit);
    await panel.getByRole('button', { name: '重新检测' }).click();
    await expect(panel).toContainText('本机 Git 可用');
    await expect(panel.getByRole('button', { name: '前往 Git 官网安装或升级' })).toHaveCount(0);
    expect(await readCatalog()).toEqual(Array<boolean>(7).fill(true));
    const reads = await page.evaluate(async (selected_root) => {
      const workspace = await window.aura.v1.createWorkspace({
        display_name: 'B1 Git',
        selected_root,
      });
      if (!workspace.ok) throw new Error(workspace.error.message);
      const workspace_id = workspace.value.workspace.workspace_id;
      const session = await window.aura.v1.createSession({ workspace_id, title: 'B1 Git' });
      if (!session.ok) throw new Error(session.error.message);
      const session_id = session.value.session_id;
      const status = await window.aura.v1.git.read({
        workspace_id,
        session_id,
        query: { kind: 'status', input: {} },
      });
      const diff = await window.aura.v1.git.read({
        workspace_id,
        session_id,
        query: { kind: 'diff', input: {} },
      });
      const log = await window.aura.v1.git.read({
        workspace_id,
        session_id,
        query: { kind: 'log', input: {} },
      });
      if (!status.ok || !diff.ok || !log.ok) throw new Error(JSON.stringify({ status, diff, log }));
      return { status: status.value, diff: diff.value, log: log.value };
    }, work);
    expect(reads.status.kind).toBe('status');
    expect(reads.diff.kind).toBe('diff');
    expect(reads.log.kind).toBe('log');
    if (reads.status.kind !== 'status' || reads.diff.kind !== 'diff' || reads.log.kind !== 'log')
      throw new Error('Git result discriminator');
    expect(reads.status.result.branch).toBe('main');
    expect(reads.status.result.entries.map((entry) => entry.path)).toContain('tracked.txt');
    expect(reads.diff.result.patch).toContain('+after');
    expect(reads.log.result.commits[0]?.subject).toBe('B1 fixture');
    expect(pageErrors).toEqual([]);
  } finally {
    await app.close();
  }
});
