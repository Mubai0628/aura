/**
 * The files a panel page may load: only what is inside its plugin's `panel/` folder, read where
 * it is now. A link, a mount point or `..` anywhere on the way is refused, and so is a name
 * Windows would read as something else (a device, a trailing dot or space, a stream). A file with
 * more than one name is refused too: a hard link would serve a file kept elsewhere, and a
 * development plugin's panel files change without being trusted again.
 */
import type { BigIntStats } from 'node:fs';
import { lstat, open, realpath } from 'node:fs/promises';
import { extname, join, sep } from 'node:path';

const TYPES: Readonly<Record<string, string>> = {
  '.html': 'text/html; charset=utf-8',
  '.htm': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.map': 'application/json; charset=utf-8',
  '.txt': 'text/plain; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.otf': 'font/otf',
};

/** The largest single file a panel loads. */
export const MAX_PANEL_FILE_BYTES = 16 * 1024 * 1024;
const MAX_DEPTH = 16;
const DEVICE = /^(?:con|prn|aux|nul|com[0-9¹²³]|lpt[0-9¹²³]|conin\$|conout\$)(?:\..*)?$/iu;

export interface PanelFile {
  body: Buffer;
  type: string;
}

/** The parts of a page address under `panel/`, or undefined when it names nothing there. */
export function panelSegments(pathname: string): string[] | undefined {
  if (!pathname.startsWith('/')) return undefined;
  const raw = pathname.slice(1).split('/');
  if (raw.length === 0 || raw.length > MAX_DEPTH) return undefined;
  const segments: string[] = [];
  for (const part of raw) {
    let name: string;
    try {
      name = decodeURIComponent(part);
    } catch {
      return undefined;
    }
    if (
      name === '' ||
      name === '.' ||
      name === '..' ||
      // eslint-disable-next-line no-control-regex -- control characters are what is refused
      /[\\/:*?"<>|\u0000-\u001f]/u.test(name) ||
      name.endsWith('.') ||
      name.endsWith(' ') ||
      DEVICE.test(name)
    )
      return undefined;
    segments.push(name);
  }
  return segments;
}

const servable = (stats: BigIntStats) =>
  stats.isFile() && stats.nlink <= 1n && stats.size <= BigInt(MAX_PANEL_FILE_BYTES);

/** What `pathname` names inside `root` (the plugin's `panel/` folder), or undefined. */
export async function panelFile(root: string, pathname: string): Promise<PanelFile | undefined> {
  const segments = panelSegments(pathname);
  const type = segments && TYPES[extname(segments.at(-1) ?? '').toLowerCase()];
  if (segments === undefined || type === undefined) return undefined;
  try {
    const top = await lstat(root);
    if (top.isSymbolicLink() || !top.isDirectory()) return undefined;
    let path = root;
    let checked: BigIntStats | undefined;
    for (const [index, name] of segments.entries()) {
      path = join(path, name);
      const found = await lstat(path, { bigint: true });
      if (found.isSymbolicLink()) return undefined;
      const last = index === segments.length - 1;
      if (last ? !servable(found) : !found.isDirectory()) return undefined;
      if (last) checked = found;
    }
    const [real, realRoot] = await Promise.all([realpath(path), realpath(root)]);
    if (checked === undefined || !real.toLowerCase().startsWith(`${realRoot.toLowerCase()}${sep}`))
      return undefined;
    const handle = await open(real, 'r');
    try {
      // The file read is the one checked: nothing was swapped in on the way since.
      const opened = await handle.stat({ bigint: true });
      if (opened.dev !== checked.dev || opened.ino !== checked.ino || !servable(opened))
        return undefined;
      const body = await handle.readFile();
      return body.length > MAX_PANEL_FILE_BYTES ? undefined : { body, type };
    } finally {
      await handle.close();
    }
  } catch {
    return undefined;
  }
}
