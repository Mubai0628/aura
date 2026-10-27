import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { existsSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, expect, test, vi } from 'vitest';
import type { AppUpdateState } from '../../shared/app-updates.js';
import { packageName } from './feed.js';
import { AppUpdates } from './service.js';

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

const listing = [
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
];

const fakeFetch = async (url: string): Promise<Response> =>
  url.endsWith('/pkg')
    ? new Response(PACKAGE, { status: 200 })
    : new Response(JSON.stringify(listing), { status: 200 });

function service(
  install: Awaited<ReturnType<typeof installation>>,
  runs: string[][],
  restarts: number[],
) {
  const published: AppUpdateState[] = [];
  const updates = new AppUpdates({
    installation: install,
    current: install.current,
    fetch: fakeFetch,
    feedUrl: 'https://api.example.test/releases',
    userAgent: 'Aura/test',
    publish: (state) => published.push(state),
    requestRestart: () => restarts.push(1),
    run: async (_program, args) => {
      runs.push([...args]);
      return args[0] === '--roll-back'
        ? '{"waiting":"0.1.0"}\n'
        : '{"waiting":"0.1.1","signature":"Verified"}\n';
    },
  });
  return { updates, published };
}

test('a test release’s own listing offers the package it serves itself over http', async () => {
  const install = await installation({ active: '0.1.0', previous: null, pending: null });
  const feedUrl = 'http://127.0.0.1:47123/releases';
  const served = [
    {
      ...listing[0],
      assets: [
        {
          name: packageName('0.1.1'),
          browser_download_url: 'http://127.0.0.1:47123/download/pkg',
          size: PACKAGE.length,
        },
      ],
    },
  ];
  const updates = (testFeed: boolean) =>
    new AppUpdates({
      installation: install,
      current: install.current,
      fetch: async () => new Response(JSON.stringify(served), { status: 200 }),
      feedUrl,
      testFeed,
      userAgent: 'Aura/test',
      publish: () => undefined,
      requestRestart: () => undefined,
    });
  expect((await updates(true).handle({ command: 'check' })).latest?.version).toBe('0.1.1');
  expect((await updates(false).handle({ command: 'check' })).phase).toBe('up_to_date');
});

// After 重启并更新 the new version first runs on trial: the record still names the old one as
// active and this one as waiting until the launcher sees this window ready and records it.
test('a version running on trial waits for nothing and learns its previous version once recorded', async () => {
  const install = await installation({ active: '0.1.0', previous: null, pending: '0.1.1' });
  const published: AppUpdateState[] = [];
  const updates = new AppUpdates({
    installation: { ...install, current: '0.1.1' },
    current: '0.1.1',
    fetch: fakeFetch,
    feedUrl: 'https://api.example.test/releases',
    userAgent: 'Aura/test',
    publish: (state) => published.push(state),
    requestRestart: () => undefined,
  });
  try {
    const trial = await updates.handle({ command: 'state' });
    expect(trial).toMatchObject({ waiting: null, previous: null, phase: 'idle' });
    await writeFile(
      join(install.root, 'active.json'),
      JSON.stringify({ active: '0.1.1', previous: '0.1.0', pending: null }),
    );
    await vi.waitFor(() => expect(published.at(-1)?.previous).toBe('0.1.0'), {
      timeout: 5_000,
      interval: 50,
    });
    expect(updates.state).toMatchObject({ current: '0.1.1', waiting: null, previous: '0.1.0' });
  } finally {
    updates.stop();
  }
});

test('a check the network breaks says so in words the page can show', async () => {
  const install = await installation({ active: '0.1.0', previous: null, pending: null });
  const updates = new AppUpdates({
    installation: install,
    current: install.current,
    fetch: async () => {
      throw new TypeError('net::ERR_INTERNET_DISCONNECTED');
    },
    feedUrl: 'https://api.example.test/releases',
    userAgent: 'Aura/test',
    publish: () => undefined,
    requestRestart: () => undefined,
  });
  const state = await updates.handle({ command: 'check' });
  expect(state.phase).toBe('error');
  expect(state.error).toMatch(/^检查更新失败.*点“检查更新”重试/u);
  expect(state.error).not.toMatch(/ERR_/u);
});

test('a download the network breaks part way keeps what arrived, says so and continues from there', async () => {
  const install = await installation({ active: '0.1.0', previous: null, pending: null });
  const ranges: (string | null)[] = [];
  let broken = true;
  const fetch = async (url: string, init?: RequestInit): Promise<Response> => {
    if (!url.endsWith('/pkg')) return new Response(JSON.stringify(listing), { status: 200 });
    const range = new Headers(init?.headers).get('range');
    ranges.push(range);
    if (!broken)
      return new Response(PACKAGE.subarray(Number(/\d+/u.exec(range!)![0])), { status: 206 });
    broken = false;
    let pulls = 0;
    const body = new ReadableStream<Uint8Array>({
      pull(controller) {
        if (pulls++ === 0) controller.enqueue(new Uint8Array(PACKAGE.subarray(0, 8)));
        else controller.error(new TypeError('net::ERR_CONNECTION_RESET'));
      },
    });
    return new Response(body, { status: 200 });
  };
  const runs: string[][] = [];
  const updates = new AppUpdates({
    installation: install,
    current: install.current,
    fetch,
    feedUrl: 'https://api.example.test/releases',
    userAgent: 'Aura/test',
    publish: () => undefined,
    requestRestart: () => undefined,
    run: async (_program, args) => {
      runs.push([...args]);
      return '{"waiting":"0.1.1"}\n';
    },
  });
  await updates.handle({ command: 'check' });
  const stopped = await updates.handle({ command: 'download' });
  expect(stopped).toMatchObject({ phase: 'paused', progress: { done: 8, total: PACKAGE.length } });
  expect(stopped.error).toMatch(/下载中断/u);
  expect(stopped.error).toMatch(/继续下载/u);
  expect(stopped.error).not.toMatch(/安装程序|ERR_/u);
  expect(runs).toEqual([]);
  const ready = await updates.handle({ command: 'download' });
  expect(ready).toMatchObject({ phase: 'ready', waiting: '0.1.1', error: null });
  expect(ranges).toEqual([null, 'bytes=8-']);
});

test('a newer release is found, downloaded, applied by the setup program and waits for a restart', async () => {
  const install = await installation({ active: '0.1.0', previous: null, pending: null });
  const runs: string[][] = [];
  const restarts: number[] = [];
  const { updates } = service(install, runs, restarts);
  await updates.start();
  updates.stop();
  expect((await updates.handle({ command: 'check' })).phase).toBe('available');
  expect(updates.state.latest).toMatchObject({
    version: '0.1.1',
    notes: 'fixes',
    size: PACKAGE.length,
  });
  const ready = await updates.handle({ command: 'download' });
  expect(ready).toMatchObject({ phase: 'ready', waiting: '0.1.1', progress: null });
  expect(runs[0]?.[0]).toBe('--apply-update');
  expect(runs[0]?.[1]).toMatch(/Aura-Update-0\.1\.1-x64\.aura-update$/u);
  expect(existsSync(runs[0]![1]!)).toBe(false);
  await updates.handle({ command: 'restart' });
  expect(restarts).toHaveLength(1);
});

// The setup program checks the package against its signed manifest; a damaged one reused on
// every try would fail until the user reinstalled.
test('a package the setup program refuses is deleted and downloaded again next time, its reason logged', async () => {
  const install = await installation({ active: '0.1.0', previous: null, pending: null });
  const downloads: string[] = [];
  const logged: [string, Record<string, unknown>][] = [];
  const outputs = [
    '{"error":"the package does not match its signed manifest"}\n',
    '{"waiting":"0.1.1"}\n',
  ];
  const updates = new AppUpdates({
    installation: install,
    current: install.current,
    fetch: async (url) => {
      if (url.endsWith('/pkg')) downloads.push(url);
      return fakeFetch(url);
    },
    feedUrl: 'https://api.example.test/releases',
    userAgent: 'Aura/test',
    publish: () => undefined,
    requestRestart: () => undefined,
    run: async () => outputs.shift()!,
    log: (event, fields) => logged.push([event, fields]),
  });
  await updates.handle({ command: 'check' });
  const refused = await updates.handle({ command: 'download' });
  expect(refused.phase).toBe('error');
  expect(refused.error).toMatch(/已删除.*重新下载/u);
  expect(refused.error).not.toMatch(/manifest/u);
  expect(
    readdirSync(join(install.dataRoot, 'updates')).filter((name) => name.includes('aura-update')),
  ).toEqual([]);
  expect(logged).toEqual([
    [
      'app_update.setup_refused',
      { action: 'apply_update', reason: 'the package does not match its signed manifest' },
    ],
  ]);
  expect((await updates.handle({ command: 'download' })).phase).toBe('ready');
  expect(downloads).toHaveLength(2);
});

test('going back with the data from before asks the launcher for it, then restarts', async () => {
  const install = await installation({ active: '0.1.1', previous: '0.1.0', pending: null });
  const runs: string[][] = [];
  const restarts: number[] = [];
  const { updates } = service(install, runs, restarts);
  await updates.start();
  updates.stop();
  expect(updates.state.previous).toBe('0.1.0');
  await updates.handle({ command: 'roll_back', restore_data: true });
  expect(runs).toEqual([['--roll-back']]);
  expect(restarts).toHaveLength(1);
  expect(
    JSON.parse(await readFile(join(install.dataRoot, 'launcher/restore.v1.json'), 'utf8')),
  ).toEqual({
    before_version: '0.1.1',
  });
});

// The launcher copies only the stores the new version migrates; the version before cannot read
// them migrated, so going back without the copy would start a version that never gets ready.
test('after an update that copied data before it started, going back takes that data with it', async () => {
  const install = await installation({ active: '0.1.1', previous: '0.1.0', pending: null });
  await mkdir(join(install.dataRoot, 'backups', 'before-0.1.1'), { recursive: true });
  const runs: string[][] = [];
  const restarts: number[] = [];
  const { updates } = service(install, runs, restarts);
  await updates.start();
  updates.stop();
  expect(updates.state.rollback_needs_data).toBe(true);
  const refused = await updates.handle({ command: 'roll_back', restore_data: false });
  expect(refused.phase).toBe('error');
  expect(refused.error).toMatch(/恢复更新前的数据/u);
  expect(runs).toEqual([]);
  expect(restarts).toHaveLength(0);
  await updates.handle({ command: 'roll_back', restore_data: true });
  expect(runs).toEqual([['--roll-back']]);
  expect(restarts).toHaveLength(1);
});

test('without a copy from before the update, going back may keep the data as it is', async () => {
  const install = await installation({ active: '0.1.1', previous: '0.1.0', pending: null });
  const { updates } = service(install, [], []);
  await updates.start();
  updates.stop();
  expect(updates.state.rollback_needs_data).toBe(false);
});

function failingService(
  install: Awaited<ReturnType<typeof installation>>,
  restarts: number[],
  options: { feedStatus?: number; setupOutput: string },
) {
  return new AppUpdates({
    installation: install,
    current: install.current,
    fetch: async (url) =>
      options.feedStatus ? new Response('', { status: options.feedStatus }) : fakeFetch(url),
    feedUrl: 'https://api.example.test/releases',
    userAgent: 'Aura/test',
    publish: () => undefined,
    requestRestart: () => restarts.push(1),
    run: async () => options.setupOutput,
  });
}

test('a rollback the setup program refuses withdraws the data request and reports why', async () => {
  const install = await installation({ active: '0.1.1', previous: '0.1.0', pending: null });
  const restarts: number[] = [];
  const updates = failingService(install, restarts, {
    setupOutput: '{"error":"上一版本的文件不完整"}\n',
  });
  await updates.start();
  updates.stop();
  const state = await updates.handle({ command: 'roll_back', restore_data: true });
  expect(state).toMatchObject({ phase: 'error', error: '上一版本的文件不完整' });
  expect(restarts).toHaveLength(0);
  // A request left behind would put the old data back at a later rollback made without it.
  expect(existsSync(join(install.dataRoot, 'launcher/restore.v1.json'))).toBe(false);
});

test('a rollback without the data withdraws a request an earlier attempt left', async () => {
  const install = await installation({ active: '0.1.1', previous: '0.1.0', pending: null });
  await mkdir(join(install.dataRoot, 'launcher'), { recursive: true });
  await writeFile(
    join(install.dataRoot, 'launcher/restore.v1.json'),
    JSON.stringify({ before_version: '0.1.1' }),
  );
  const restarts: number[] = [];
  const { updates } = service(install, [], restarts);
  await updates.start();
  updates.stop();
  await updates.handle({ command: 'roll_back', restore_data: false });
  expect(restarts).toHaveLength(1);
  expect(existsSync(join(install.dataRoot, 'launcher/restore.v1.json'))).toBe(false);
});

test('a rollback that goes through answers with the version waiting, not an earlier error', async () => {
  const install = await installation({ active: '0.1.1', previous: '0.1.0', pending: null });
  const restarts: number[] = [];
  const updates = failingService(install, restarts, {
    feedStatus: 500,
    setupOutput: '{"waiting":"0.1.0"}\n',
  });
  await updates.start();
  updates.stop();
  expect((await updates.handle({ command: 'check' })).phase).toBe('error');
  const state = await updates.handle({ command: 'roll_back', restore_data: false });
  expect(state).toMatchObject({ phase: 'ready', waiting: '0.1.0', error: null });
  expect(restarts).toHaveLength(1);
});

test('a version the launcher put back is shown until dismissed, with the data offer when going back', async () => {
  const install = await installation({ active: '0.1.1', previous: '0.1.0', pending: null });
  await mkdir(join(install.dataRoot, 'launcher'), { recursive: true });
  await mkdir(join(install.dataRoot, 'backups', 'before-0.1.1'), { recursive: true });
  await writeFile(
    join(install.dataRoot, 'launcher/trial-failed.v1.json'),
    JSON.stringify({
      version: '0.1.0',
      kept_version: '0.1.1',
      failure: 'crashed',
      exit_code: 1,
      detail: 'exit code 1',
      at: 'x',
    }),
  );
  const { updates } = service(install, [], []);
  await updates.start();
  updates.stop();
  expect(updates.state.notice).toEqual({
    version: '0.1.0',
    kept_version: '0.1.1',
    detail: 'exit code 1',
    going_back: true,
    data_restorable: true,
  });
  await updates.handle({ command: 'dismiss_notice' });
  expect(updates.state.notice).toBeNull();
  expect(existsSync(join(install.dataRoot, 'launcher/trial-failed.v1.json'))).toBe(false);
});

test('an update applied before the window closed is ready to restart into on the next start', async () => {
  const install = await installation({ active: '0.1.0', previous: null, pending: '0.1.1' });
  const { updates } = service(install, [], []);
  await updates.start();
  updates.stop();
  expect(updates.state).toMatchObject({ phase: 'ready', waiting: '0.1.1' });
});

test('without an installation that updates itself nothing is offered', async () => {
  const updates = new AppUpdates({
    installation: null,
    current: '0.0.0-dev',
    fetch: fakeFetch,
    feedUrl: 'https://api.example.test/releases',
    userAgent: 'Aura/test',
    publish: () => undefined,
    requestRestart: () => undefined,
  });
  await updates.start();
  expect((await updates.handle({ command: 'check' })).phase).toBe('idle');
  expect(updates.state.available).toBe(false);
});
