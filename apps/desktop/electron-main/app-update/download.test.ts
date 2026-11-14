import { createServer, type Server } from 'node:http';
import { existsSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, expect, test } from 'vitest';
import { downloadPackage } from './download.js';

const BODY = Buffer.alloc(3 * 1024 * 1024, 7).map((_, index) => index % 251);
const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  await Promise.all(cleanups.splice(0).map((cleanup) => cleanup()));
});

async function folder(): Promise<string> {
  const path = await mkdtemp(join(tmpdir(), 'aura-update-download-'));
  cleanups.push(() => rm(path, { recursive: true, force: true }));
  return path;
}

/** Serves BODY; honours Range unless `ranges` is false. Records the Range headers it saw. */
async function serve(ranges = true): Promise<{ url: string; seen: (string | undefined)[] }> {
  const seen: (string | undefined)[] = [];
  const server: Server = createServer((request, response) => {
    seen.push(request.headers.range);
    const start = ranges ? Number(/bytes=(\d+)-/u.exec(request.headers.range ?? '')?.[1] ?? 0) : 0;
    response.writeHead(start > 0 ? 206 : 200, { 'content-length': String(BODY.length - start) });
    response.end(BODY.subarray(start));
  });
  await new Promise<void>((ready) => server.listen(0, '127.0.0.1', ready));
  cleanups.push(() => new Promise<void>((closed) => server.close(() => closed())));
  const address = server.address();
  if (address === null || typeof address === 'string') throw new Error('no address');
  return { url: `http://127.0.0.1:${address.port}/package`, seen };
}

test('a stopped download continues from where it stopped and ends with the whole package', async () => {
  const { url, seen } = await serve();
  const target = await folder();
  const stopped = new AbortController();
  await expect(
    downloadPackage({
      fetch,
      url,
      size: BODY.length,
      folder: target,
      name: 'Aura-Update-0.1.1-x64.aura-update',
      signal: stopped.signal,
      onProgress: (done) => {
        if (done > 1024 * 1024) stopped.abort();
      },
    }),
  ).rejects.toThrow();
  const part = await stat(join(target, 'Aura-Update-0.1.1-x64.aura-update.part'));
  expect(part.size).toBeGreaterThan(0);
  const file = await downloadPackage({
    fetch,
    url,
    size: BODY.length,
    folder: target,
    name: 'Aura-Update-0.1.1-x64.aura-update',
    signal: new AbortController().signal,
    onProgress: () => undefined,
  });
  expect(Buffer.compare(await readFile(file), BODY)).toBe(0);
  expect(seen[1]).toBe(`bytes=${part.size}-`);
});

test('a server that ignores ranges is read from the start, and a wrong size is refused', async () => {
  const { url } = await serve(false);
  const target = await folder();
  const name = 'Aura-Update-0.1.2-x64.aura-update';
  const stopped = new AbortController();
  await downloadPackage({
    fetch,
    url,
    size: BODY.length,
    folder: target,
    name,
    signal: stopped.signal,
    onProgress: (done) => {
      if (done > 1024 * 1024) stopped.abort();
    },
  }).catch(() => undefined);
  const file = await downloadPackage({
    fetch,
    url,
    size: BODY.length,
    folder: target,
    name,
    signal: new AbortController().signal,
    onProgress: () => undefined,
  });
  expect(Buffer.compare(await readFile(file), BODY)).toBe(0);
  await expect(
    downloadPackage({
      fetch,
      url,
      size: BODY.length - 1,
      folder: target,
      name: 'Aura-Update-0.1.3-x64.aura-update',
      signal: new AbortController().signal,
      onProgress: () => undefined,
    }),
  ).rejects.toThrow(/比更新包大/u);
});

test('a part that cannot be written ends the download with the system’s code instead of waiting', async () => {
  const { url } = await serve();
  const target = await folder();
  const name = 'Aura-Update-0.1.4-x64.aura-update';
  // A folder where the part goes cannot be opened for writing.
  await mkdir(join(target, `${name}.part`));
  await expect(
    downloadPackage({
      fetch,
      url,
      size: BODY.length,
      folder: target,
      name,
      signal: new AbortController().signal,
      onProgress: () => undefined,
    }),
  ).rejects.toMatchObject({ code: expect.stringMatching(/^E[A-Z]+$/u) });
});

test('a whole package an earlier download could not rename is used without asking again', async () => {
  const { url, seen } = await serve();
  const target = await folder();
  const name = 'Aura-Update-0.1.5-x64.aura-update';
  await writeFile(join(target, `${name}.part`), BODY);
  const file = await downloadPackage({
    fetch,
    url,
    size: BODY.length,
    folder: target,
    name,
    signal: new AbortController().signal,
    onProgress: () => undefined,
  });
  expect(Buffer.compare(await readFile(file), BODY)).toBe(0);
  expect(seen).toEqual([]);
});

test('a package GitHub does not give says why in words, and a part it does not match is dropped', async () => {
  const target = await folder();
  const name = 'Aura-Update-0.1.6-x64.aura-update';
  const part = join(target, `${name}.part`);
  await writeFile(part, BODY.subarray(0, 10));
  const answered = (status: number) =>
    downloadPackage({
      fetch: async () => new Response('', { status }),
      url: 'https://example.test/package',
      size: BODY.length,
      folder: target,
      name,
      signal: new AbortController().signal,
      onProgress: () => undefined,
    });
  await expect(answered(416)).rejects.toThrow(
    '已下载的部分和 GitHub 上的更新包对不上，已删掉；点“继续下载”会从头重新下载。',
  );
  expect(existsSync(part)).toBe(false);
  await expect(answered(404)).rejects.toThrow(
    '这个版本的更新包在 GitHub 上找不到了，这次没下载；请稍后重新检查更新。',
  );
  await expect(answered(503)).rejects.toThrow(
    'GitHub 暂时不可用，下载暂停了；已下载的部分会保留，稍后点“继续下载”。',
  );
  await expect(answered(429)).rejects.toThrow(
    'GitHub 暂时限制了下载（同一网络请求太频繁），下载暂停了；已下载的部分会保留，约一小时后点“继续下载”。',
  );
  await expect(answered(418)).rejects.toThrow(
    'GitHub 没有给出更新包，下载暂停了；已下载的部分会保留，稍后点“继续下载”，仍然不行请重新检查更新。',
  );
});
