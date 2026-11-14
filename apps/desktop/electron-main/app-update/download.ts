import { once } from 'node:events';
import { createWriteStream } from 'node:fs';
import { mkdir, rename, rm, stat } from 'node:fs/promises';
import { join } from 'node:path';

export type FetchFunction = (url: string, init?: RequestInit) => Promise<Response>;

/** A package GitHub did not serve, in words that already say what to do next. */
export class DownloadRefused extends Error {}

const sizeOf = (path: string): Promise<number | null> =>
  stat(path).then(
    (info) => info.size,
    () => null,
  );

/** Why GitHub did not serve the package, by the status it answered. */
function refused(status: number): DownloadRefused {
  if (status === 404 || status === 410)
    return new DownloadRefused(
      '这个版本的更新包在 GitHub 上找不到了，这次没下载；请稍后重新检查更新。',
    );
  if (status === 403 || status === 429)
    return new DownloadRefused(
      'GitHub 暂时限制了下载（同一网络请求太频繁），下载暂停了；已下载的部分会保留，约一小时后点“继续下载”。',
    );
  if (status >= 500)
    return new DownloadRefused(
      'GitHub 暂时不可用，下载暂停了；已下载的部分会保留，稍后点“继续下载”。',
    );
  return new DownloadRefused(
    'GitHub 没有给出更新包，下载暂停了；已下载的部分会保留，稍后点“继续下载”，仍然不行请重新检查更新。',
  );
}

/**
 * Downloads `url` into `folder` as `name`, continuing a `.part` file an earlier, stopped download
 * left. Resolves with the finished file once it has exactly `size` bytes; `signal` stops it and
 * keeps what arrived. A part that cannot be written fails it with the system's error code.
 */
export async function downloadPackage(options: {
  fetch: FetchFunction;
  url: string;
  size: number;
  folder: string;
  name: string;
  signal: AbortSignal;
  onProgress: (done: number, total: number) => void;
}): Promise<string> {
  const finished = join(options.folder, options.name);
  if ((await sizeOf(finished)) === options.size) return finished;
  await mkdir(options.folder, { recursive: true });
  const part = `${finished}.part`;
  let start = (await sizeOf(part)) ?? 0;
  // A whole package an earlier download could not rename: asking for more would be refused.
  if (start === options.size) {
    await rename(part, finished);
    return finished;
  }
  if (start > options.size) {
    await rm(part, { force: true });
    start = 0;
  }
  const response = await options.fetch(options.url, {
    headers: start > 0 ? { Range: `bytes=${start}-` } : {},
    signal: options.signal,
  });
  if (response.status === 416) {
    // The part does not belong to the package GitHub serves now; the next try starts over.
    await rm(part, { force: true });
    throw new DownloadRefused(
      '已下载的部分和 GitHub 上的更新包对不上，已删掉；点“继续下载”会从头重新下载。',
    );
  }
  if (response.status === 200) start = 0;
  else if (response.status !== 206) throw refused(response.status);
  if (!response.body) throw new Error('下载失败：没有收到内容');
  const output = createWriteStream(part, { flags: start > 0 ? 'a' : 'w' });
  // A part that cannot be written (a full disk, a file another program holds) ends the download
  // with the system's error instead of leaving it waiting for the file.
  let failure: unknown;
  const failed = new Promise<never>((_, fail) =>
    output.on('error', (error) => {
      failure ??= error;
      fail(error);
    }),
  );
  failed.catch(() => undefined);
  const reader = response.body.getReader();
  let done = start;
  options.onProgress(done, options.size);
  try {
    for (;;) {
      const { done: ended, value } = await Promise.race([reader.read(), failed]);
      if (ended) break;
      done += value.byteLength;
      if (done > options.size) throw new Error('下载的内容比更新包大');
      if (!output.write(value)) await Promise.race([once(output, 'drain'), failed]);
      options.onProgress(done, options.size);
    }
  } finally {
    void reader.cancel().catch(() => undefined);
    // What arrived is written out before the download ends, also when it was stopped.
    await Promise.race([new Promise<void>((closed) => output.end(() => closed())), failed]).catch(
      () => undefined,
    );
  }
  if (failure !== undefined) throw failure;
  if ((await sizeOf(part)) !== options.size) throw new Error('下载不完整');
  await rename(part, finished);
  return finished;
}
