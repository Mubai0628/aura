import { expect, test, type Page } from '@playwright/test';
import { copyFile, mkdir, mkdtemp, readFile, stat, writeFile } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { launch, finished, submit } from './s06/harness';
import { armReadOnlyWrite } from './s06/setup';
import { networkProvider } from './s07/provider';

async function repository() {
  const root = await mkdtemp(join(tmpdir(), 'aura-git-test-review-'));
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
  const select = (git_path: string) =>
    writeFile(profile, JSON.stringify({ global_config: global, system_config: system, git_path }));
  await select(image);
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
        HTTP_PROXY: '',
        HTTPS_PROXY: '',
        ALL_PROXY: '',
        NO_PROXY: '*',
      },
    });
  await git(['init', '--initial-branch=main']);
  await writeFile(join(workspace, 'stage.txt'), 'before\n');
  await writeFile(join(workspace, 'unstaged.txt'), 'before\n');
  await writeFile(join(workspace, 'large.txt'), 'before\n');
  await writeFile(join(workspace, '.gitattributes'), '*.txt filter=Bad diff=Bad\n');
  await git(['add', '--all']);
  await git(['commit', '-m', 'temporary review base']);
  return { root, workspace, profile, select, git };
}
async function session(page: Page, workspace: string) {
  const id = await page.evaluate(async (selected_root) => {
    const workspace = await window.aura.v1.createWorkspace({
      display_name: 'L03 review workspace',
      selected_root,
    });
    if (!workspace.ok) throw Error(workspace.error.message);
    const created = await window.aura.v1.createSession({
      workspace_id: workspace.value.workspace.workspace_id,
      title: 'Git 审查场景',
    });
    if (!created.ok) throw Error(created.error.message);
    return created.value.session_id;
  }, workspace);
  await page.locator('.session-row').filter({ hasText: 'Git 审查场景' }).click();
  await expect(page.getByRole('heading', { name: 'Git 审查场景', exact: true })).toBeVisible();
  return id;
}

async function launchReview(root: string, profile: string) {
  const fixture = await launch(root, { AURA_GIT_TEST_PROFILE: profile }, undefined, [
    '--disable-features=CalculateNativeWinOcclusion',
    '--disable-background-timer-throttling',
    '--disable-renderer-backgrounding',
  ]);
  await fixture.app.evaluate(({ BrowserWindow }) => {
    // Measure the interactive page, independent of which desktop window owns focus.
    BrowserWindow.getAllWindows()[0]?.webContents.setBackgroundThrottling(false);
  });
  return fixture;
}

// eslint-disable-next-line no-empty-pattern -- Electron supplies its own page.
test('review crosses the isolated desktop boundary for groups, counts, binary and paged patches and refreshes after a read tool', async ({}, info) => {
  test.setTimeout(240_000);
  const f = await repository();
  await writeFile(join(f.workspace, 'stage.txt'), 'staged one\nstaged two\n');
  await f.git(['add', 'stage.txt']);
  await writeFile(join(f.workspace, 'unstaged.txt'), 'working tree\n');
  await writeFile(
    join(f.workspace, 'large.txt'),
    Array.from({ length: 16000 }, (_, index) => `line-${index}\n`).join(''),
  );
  await writeFile(join(f.workspace, '新增 文件.txt'), 'new one\nnew two\n');
  await writeFile(join(f.workspace, 'new.bin'), Buffer.from([0, 1, 255]));
  for (const key of [
    'filter.Bad.clean',
    'filter.Bad.smudge',
    'filter.Bad.process',
    'diff.Bad.textconv',
    'diff.Bad.command',
    'core.fsmonitor',
    'core.pager',
  ])
    await f.git(['config', key, 'echo forbidden > program.marker']);
  await f.git(['config', 'filter.Bad.required', 'true']);
  const index = join(f.workspace, '.git/index');
  const before = { bytes: await readFile(index), mtime: (await stat(index)).mtimeMs };
  const provider = await networkProvider();
  provider.setAction('fs.read', { path: 'unstaged.txt' });
  const { app, page } = await launchReview(f.root, f.profile);
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  try {
    const setup = await armReadOnlyWrite(page, f.workspace, provider.url, {
      title: 'L03 review',
      text: '读取工作区文件。',
    });
    expect(setup.error).toBeUndefined();
    const sessionId = setup.sessionId as string;
    await finished(page, sessionId);
    await page.keyboard.press('Control+Shift+G');
    const review = page.getByRole('region', { name: 'Git 审查', exact: true });
    const card = (path: string) =>
      review.getByRole('article', { name: `${path} 的改动`, exact: true });
    // The uncommitted changes, staged or not, and the untracked files, in one list; each
    // file's lines read on their own.
    await expect(card('stage.txt').getByLabel('+2 −1', { exact: true })).toBeVisible({
      timeout: 30_000,
    });
    await expect(card('stage.txt')).toContainText('staged two', { timeout: 30_000 });
    // Each Git read takes about a second here; the files' diffs come two at a time.
    await expect(card('unstaged.txt')).toContainText('working tree', { timeout: 30_000 });
    await expect(card('新增 文件.txt')).toContainText('new two', { timeout: 30_000 });
    await expect(card('新增 文件.txt').getByText('新文件', { exact: true })).toBeVisible();
    await expect(card('新增 文件.txt')).toContainText('未运行外部程序，按原始内容比较');
    await expect(card('new.bin')).toContainText('二进制文件，不显示差异。', { timeout: 30_000 });
    await expect(card('large.txt')).toContainText('line-15999', { timeout: 30_000 });
    // Side by side, for real.
    await review.getByRole('button', { name: '更多审查选项' }).click();
    await review.getByRole('menuitemcheckbox', { name: '左右对照' }).click();
    await page.keyboard.press('Escape');
    await expect(card('stage.txt').locator('table[data-mode="split"]')).toBeVisible();
    // 全部改动 against main, from main itself, is the same set of files.
    await review.getByRole('button', { name: /未提交的改动/u }).click();
    await review.getByRole('menuitemradio', { name: '全部改动（与 main 比较）' }).click();
    await expect(review.getByRole('button', { name: /全部改动 · 与 main 比较/u })).toBeVisible();
    await expect(card('stage.txt').getByLabel('+2 −1', { exact: true })).toBeVisible({
      timeout: 30_000,
    });
    await writeFile(join(f.workspace, 'manual.txt'), 'manual refresh\n');
    await expect(card('manual.txt')).toHaveCount(0);
    await review.getByRole('button', { name: '更多审查选项' }).click();
    await review.getByRole('menuitem', { name: '刷新' }).click();
    await expect(card('manual.txt')).toBeVisible({ timeout: 30_000 });
    await writeFile(join(f.workspace, 'after-read-tool.txt'), 'read tool completion refresh\n');
    await expect(card('after-read-tool.txt')).toHaveCount(0);
    await submit(page, sessionId, '再次读取文件，完成后刷新审查。');
    await finished(page, sessionId);
    await expect(card('after-read-tool.txt')).toBeVisible({ timeout: 30_000 });
    // Opened again, the page shows what it read at once — the same
    // comparison and files, no "正在读取 Git…" — and checks Git behind it.
    await page.getByRole('button', { name: '打开工具', exact: true }).click();
    await page
      .getByRole('menu', { name: '选择工具' })
      .getByRole('menuitem', { name: /^文件/u })
      .click();
    await expect(review).toHaveCount(0);
    await page.getByRole('tab', { name: /^审查/u }).click();
    await expect(card('after-read-tool.txt')).toBeVisible({ timeout: 1_000 });
    await expect(review.getByRole('button', { name: /全部改动 · 与 main 比较/u })).toBeVisible();
    await expect(review.locator('.review-loading')).toHaveCount(0);
    expect(await readFile(index)).toEqual(before.bytes);
    expect((await stat(index)).mtimeMs).toBe(before.mtime);
    expect(
      await stat(join(f.workspace, 'program.marker')).then(
        () => true,
        () => false,
      ),
    ).toBe(false);
    expect(errors).toEqual([]);
    expect(provider.errors).toEqual([]);
    await card('stage.txt').scrollIntoViewIfNeeded();
    await page.screenshot({ path: info.outputPath('review.png'), animations: 'disabled' });
  } finally {
    await app.close();
    await provider.close();
  }
});

test('review explains old or missing Git and recovers after explicit redetection', async () => {
  test.setTimeout(120_000);
  const f = await repository();
  const fake = join(f.root, 'git.exe');
  await copyFile(
    resolve(import.meta.dirname, '../../../target/debug/aura-git-version-fixture.exe'),
    fake,
  );
  await writeFile(join(f.root, 'version.txt'), 'git version 2.39.0.windows.1\n');
  await f.select(fake);
  const { app, page } = await launchReview(f.root, f.profile);
  try {
    await session(page, f.workspace);
    await page.keyboard.press('Control+Shift+G');
    const review = page.getByRole('region', { name: 'Git 审查', exact: true });
    await expect(review).toContainText('本机 Git 版本低于 2.40');
    await expect(review.getByRole('button', { name: '打开 Git 安装页面' })).toBeVisible();
    const refresh = async () => {
      await review.getByRole('button', { name: '更多审查选项' }).click();
      await review.getByRole('menuitem', { name: '刷新' }).click();
    };
    await f.select(join(f.root, 'missing-git.exe'));
    await refresh();
    await expect(review).toContainText('未找到本机 Git');
    await f.select(process.env.AURA_GIT_TEST_BINARY ?? 'C:/Program Files/Git/cmd/git.exe');
    await refresh();
    await expect(review).toContainText('当前没有未提交的改动。', { timeout: 15_000 });
  } finally {
    await app.close();
  }
});

// eslint-disable-next-line no-empty-pattern -- Electron supplies its own page.
test('review stays interactive while Core enumerates fifty thousand temporary files', async ({}, info) => {
  test.setTimeout(180_000);
  const f = await repository();
  const files = join(f.workspace, 'files');
  await mkdir(files);
  for (let offset = 0; offset < 50000; offset += 500)
    await Promise.all(
      Array.from({ length: 500 }, (_, index) =>
        writeFile(join(files, `${String(offset + index).padStart(5, '0')}.txt`), 'fixture\n'),
      ),
    );
  const { app, page } = await launchReview(f.root, f.profile);
  try {
    await session(page, f.workspace);
    await page.evaluate(() => {
      const record = { count: 0, alive: true };
      (window as Window & { l03Frames?: typeof record }).l03Frames = record;
      const tick = () => {
        if (record.alive) {
          record.count++;
          requestAnimationFrame(tick);
        }
      };
      requestAnimationFrame(tick);
    });
    await page.keyboard.press('Control+Shift+G');
    const review = page.getByRole('region', { name: 'Git 审查', exact: true });
    await expect(review).toContainText('Git 状态共有 50000 项', { timeout: 60_000 });
    await expect(review.locator('.review-card')).toHaveCount(100);
    await expect(review.locator('.review-list-row')).toHaveCount(1000);
    expect(
      await page.evaluate(
        () => (window as Window & { l03Frames?: { count: number } }).l03Frames?.count ?? 0,
      ),
    ).toBeGreaterThan(5);
    await info.attach('temporary-root', { body: f.root, contentType: 'text/plain' });
  } finally {
    await app.close();
  }
});
