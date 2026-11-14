import { chmod, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, expect, test } from 'vitest';
import { packageName } from './feed.js';
import { AppUpdates, type AppUpdateDependencies } from './service.js';

// What the update page says when a step fails on this computer rather than on the network: a
// file that cannot be written, a package GitHub no longer has, a refusal of the setup program.

const PACKAGE = Buffer.from('the update package');
const folders: string[] = [];
afterEach(async () => {
  await Promise.all(folders.splice(0).map((path) => rm(path, { recursive: true, force: true })));
});

async function installation(state: Record<string, unknown>) {
  const root = await mkdtemp(join(tmpdir(), 'aura-update-root-'));
  const dataRoot = await mkdtemp(join(tmpdir(), 'aura-update-data-'));
  folders.push(root, dataRoot);
  await writeFile(join(root, 'active.json'), JSON.stringify(state));
  await writeFile(join(root, 'Uninstall Aura.exe'), 'setup');
  return { root, setup: join(root, 'Uninstall Aura.exe'), dataRoot, current: String(state.active) };
}

const listing = JSON.stringify([
  {
    tag_name: 'v0.1.1',
    draft: false,
    prerelease: false,
    published_at: null,
    body: 'fixes',
    assets: [
      {
        name: packageName('0.1.1'),
        browser_download_url: 'https://example.test/pkg',
        size: PACKAGE.length,
      },
    ],
  },
]);

const served = async (url: string): Promise<Response> =>
  url.endsWith('/pkg')
    ? new Response(PACKAGE, { status: 200 })
    : new Response(listing, { status: 200 });

function updates(
  install: Awaited<ReturnType<typeof installation>>,
  change: Partial<AppUpdateDependencies> = {},
): AppUpdates {
  return new AppUpdates({
    installation: install,
    current: install.current,
    fetch: served,
    feedUrl: 'https://api.example.test/releases',
    userAgent: 'Aura/test',
    publish: () => undefined,
    requestRestart: () => undefined,
    run: async (_program, args) =>
      args[0] === '--roll-back' ? '{"waiting":"0.1.0"}' : '{"waiting":"0.1.1"}',
    ...change,
  });
}

test('a part the download cannot write pauses it and says why in words', async () => {
  const install = await installation({ active: '0.1.0', previous: null, pending: null });
  const folder = join(install.dataRoot, 'updates');
  await mkdir(folder, { recursive: true });
  const part = join(folder, `${packageName('0.1.1')}.part`);
  // A read-only part cannot be opened for writing.
  await writeFile(part, '');
  await chmod(part, 0o444);
  try {
    const update = updates(install);
    await update.handle({ command: 'check' });
    expect(await update.handle({ command: 'download' })).toMatchObject({
      phase: 'paused',
      error:
        '更新包没能写入：没有写入权限，或文件被安全软件拦下了。已下载的部分会保留，稍后点“继续下载”接着下载。',
    });
  } finally {
    await chmod(part, 0o666);
  }
});

test('a package GitHub no longer has says so instead of offering to go on', async () => {
  const install = await installation({ active: '0.1.0', previous: null, pending: null });
  const update = updates(install, {
    fetch: async (url) => (url.endsWith('/pkg') ? new Response('', { status: 404 }) : served(url)),
  });
  await update.handle({ command: 'check' });
  expect((await update.handle({ command: 'download' })).error).toBe(
    '这个版本的更新包在 GitHub 上找不到了，这次没下载；请稍后重新检查更新。',
  );
});

test('a check whose result cannot be written keeps what it found and says the disk is why', async () => {
  const install = await installation({ active: '0.1.0', previous: null, pending: null });
  // A folder where the settings file goes cannot be written as a file.
  await mkdir(join(install.dataRoot, 'updates', 'settings.json'), { recursive: true });
  expect(await updates(install).handle({ command: 'check' })).toMatchObject({
    phase: 'available',
    latest: { version: '0.1.1' },
    error:
      '检查结果没能写入磁盘（磁盘已满或没有写入权限），下次启动会重新检查；请腾出空间或检查数据文件夹权限。',
  });
});

test('an installed update whose old package cannot be removed still waits for the restart', async () => {
  const install = await installation({ active: '0.1.0', previous: null, pending: null });
  // A folder named like a part cannot be removed as a file.
  await mkdir(join(install.dataRoot, 'updates', `${packageName('0.1.2')}.part`, 'inside'), {
    recursive: true,
  });
  const logged: string[] = [];
  const update = updates(install, { log: (event) => logged.push(event) });
  await update.handle({ command: 'check' });
  expect(await update.handle({ command: 'download' })).toMatchObject({
    phase: 'ready',
    waiting: '0.1.1',
    error: null,
  });
  expect(logged).toEqual(['app_update.cleanup_failed']);
});

test('a rollback refusal the setup program names is said in words, and an English one is not shown', async () => {
  const install = await installation({ active: '0.1.1', previous: '0.1.0', pending: null });
  for (const [output, words] of [
    [
      { error: 'there is no previous version to go back to', code: 'no_previous_version' },
      '找不到可以回去的上一版本（可能已被清理），没有回退；可以从 GitHub 下载旧版安装包重新安装。',
    ],
    [
      { error: 'cannot write the install state: Access is denied. (os error 5)' },
      '没能回到上一版本，当前版本照常使用；请稍后再试，仍然失败时可以从 GitHub 下载旧版安装包重新安装。',
    ],
  ] as const) {
    const update = updates(install, { run: async () => JSON.stringify(output) });
    await update.start();
    update.stop();
    expect((await update.handle({ command: 'roll_back', restore_data: false })).error).toBe(words);
  }
});

test('a package whose signature the setup program refuses is said in words and dropped', async () => {
  const install = await installation({ active: '0.1.0', previous: null, pending: null });
  const update = updates(install, {
    run: async () =>
      JSON.stringify({
        error: "this release's signature is not valid: the signer is not trusted",
        code: 'signature_invalid',
      }),
  });
  await update.handle({ command: 'check' });
  expect(await update.handle({ command: 'download' })).toMatchObject({
    phase: 'error',
    error: '这个更新包的签名不对，Aura 没有安装它，已删除这次下载的更新包；请稍后重新检查更新。',
  });
});
