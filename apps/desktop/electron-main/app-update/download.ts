import { createWriteStream } from 'node:fs';
import { mkdir, rename, rm, stat } from 'node:fs/promises';
import { join } from 'node:path';

export type FetchFunction = (url: string, init?: RequestInit) => Promise<Response>;

const sizeOf = (path: string): Promise<number | null> =>
  stat(path).then(
    (info) => info.size,
    () => null,
  );

/**
 * Downloads `url` into `folder` as `name`, continuing a `.part` file an earlier, stopped download
 * left. Resolves with the finished file once it has exactly `size` bytes; `signal` stops it and
 * keeps what arrived.
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
  if (start > options.size) {
    await rm(part, { force: true });
    start = 0;
  }
  const response = await options.fetch(options.url, {
    headers: start > 0 ? { Range: `bytes=${start}-` } : {},
    signal: options.signal,
  });
  if (response.status === 200) start = 0;
  else if (response.status !== 206) throw new Error(`下载失败（HTTP ${response.status}）`);
  if (!response.body) throw new Error('下载失败：没有收到内容');
  const output = createWriteStream(part, { flags: start > 0 ? 'a' : 'w' });
  let done = start;
  options.onProgress(done, options.size);
  try {
    const reader = response.body.getReader();
    for (;;) {
      const { done: ended, value } = await reader.read();
      if (ended) break;
      done += value.byteLength;
      if (done > options.size) throw new Error('下载的内容比更新包大');
      if (!output.write(value))
        await new Promise<void>((resume) => output.once('drain', () => resume()));
      options.onProgress(done, options.size);
    }
  } finally {
    await new Promise<void>((closed) => output.end(() => closed()));
  }
  if ((await sizeOf(part)) !== options.size) throw new Error('下载不完整');
  await rename(part, finished);
  return finished;
}
