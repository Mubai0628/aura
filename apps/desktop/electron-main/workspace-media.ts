import { randomUUID } from 'node:crypto';
import { open, stat } from 'node:fs/promises';
import { basename, extname } from 'node:path';
import { Readable } from 'node:stream';
import { ipcMain, type IpcMainInvokeEvent } from 'electron';
import {
  WORKSPACE_MEDIA,
  WORKSPACE_MEDIA_CHANNEL,
  parseWorkspaceMediaRequest,
  type WorkspaceMediaResult,
} from '../shared/workspace-media.js';
import type { CoreSupervisor } from './core-supervisor.js';
import { locateWorkspaceFile } from './open-outside.js';

/** Addresses kept: one per media file the 文件 tabs have shown, the oldest given up first. */
const KEPT = 256;
const ADDRESS = /^\/workspace-media\/([0-9a-f-]{36})\/[^/]+$/u;
/** A file loaded as a page by mistake could run nothing, here or anywhere. */
const SANDBOX = "default-src 'none'; style-src 'unsafe-inline'; sandbox";
const MEDIA_TYPES: Readonly<Record<string, string>> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  bmp: 'image/bmp',
  ico: 'image/x-icon',
  avif: 'image/avif',
  svg: 'image/svg+xml',
  mp4: 'video/mp4',
  m4v: 'video/mp4',
  webm: 'video/webm',
  ogv: 'video/ogg',
  mov: 'video/quicktime',
  mp3: 'audio/mpeg',
  wav: 'audio/wav',
  flac: 'audio/flac',
  ogg: 'audio/ogg',
  oga: 'audio/ogg',
  m4a: 'audio/mp4',
  aac: 'audio/aac',
  opus: 'audio/ogg',
};

/** A player asks for one contiguous range. Invalid and multipart ranges are refused. */
function byteRange(value: string, size: number): { start: number; end: number } | undefined {
  const match = /^bytes=(\d*)-(\d*)$/iu.exec(value);
  if (match === null || size === 0 || (match[1] === '' && match[2] === '')) return undefined;
  const first = match[1] === '' ? undefined : Number(match[1]);
  const last = match[2] === '' ? undefined : Number(match[2]);
  if (
    (first !== undefined && !Number.isSafeInteger(first)) ||
    (last !== undefined && !Number.isSafeInteger(last))
  )
    return undefined;
  if (first === undefined) {
    if (last === undefined || last === 0) return undefined;
    return { start: Math.max(0, size - last), end: size - 1 };
  }
  if (first >= size || (last !== undefined && last < first)) return undefined;
  return { start: first, end: Math.min(last ?? size - 1, size - 1) };
}

/**
 * Pictures, video and sound of a
 * conversation's workspace for its 文件 tab. Main finds the file inside the workspace (as it does
 * to open one outside Aura), refuses anything that is not media, and streams it from an address
 * of Aura's own scheme whose name is random and good for this run of Aura only.
 */
export class WorkspaceMedia {
  readonly #files = new Map<string, string>();

  install(
    core: () => Pick<CoreSupervisor, 'refreshWorkspaceSessionSnapshot'> | undefined,
    assertTrustedSender: (event: IpcMainInvokeEvent) => void,
  ): void {
    ipcMain.handle(WORKSPACE_MEDIA_CHANNEL, async (event, value: unknown) => {
      assertTrustedSender(event);
      return this.address(core(), value);
    });
  }

  async address(
    core: Pick<CoreSupervisor, 'refreshWorkspaceSessionSnapshot'> | undefined,
    value: unknown,
  ): Promise<WorkspaceMediaResult> {
    const request = parseWorkspaceMediaRequest(value);
    if (request === undefined) return { ok: false, message: '无效的文件请求' };
    if (!WORKSPACE_MEDIA.test(request.path))
      return { ok: false, message: '这个文件不是图片、视频或音频' };
    if (core === undefined) return { ok: false, message: 'Aura Core 尚未就绪，请稍后再试' };
    let target: string;
    try {
      const located = await locateWorkspaceFile(request, core);
      if ('refused' in located) return { ok: false, message: located.refused };
      target = located.target;
      // Where a link leads is what is shown: it must still be media, and a file.
      if (!WORKSPACE_MEDIA.test(target) || !(await stat(target)).isFile())
        return { ok: false, message: '这个文件不是图片、视频或音频' };
    } catch {
      return { ok: false, message: '没能打开这个文件，请重试。' };
    }
    return { ok: true, url: this.register(target) };
  }

  /** An address that streams `target`, a media file Main found for the renderer. */
  register(target: string): string {
    const token = randomUUID();
    this.#files.set(token, target);
    if (this.#files.size > KEPT) this.#files.delete(this.#files.keys().next().value!);
    return `aura://app/workspace-media/${token}/${encodeURIComponent(basename(target))}`;
  }

  /** The file an address names, streamed with the range asked for; nothing for other paths. */
  async serve(request: Request): Promise<Response | undefined> {
    const match = ADDRESS.exec(new URL(request.url).pathname);
    if (match === null) return undefined;
    const target = this.#files.get(match[1]!);
    if (target === undefined) return new Response('Not found', { status: 404 });
    // Hold the same opened file for the size and stream. Electron's file:// fetch omits the
    // range response headers, so Chromium cannot establish a seekable timeline from it.
    const file = await open(target, 'r');
    try {
      const { size } = await file.stat();
      const headers = new Headers({
        'Content-Security-Policy': SANDBOX,
        'Cache-Control': 'no-store',
        'X-Content-Type-Options': 'nosniff',
        'Content-Type':
          MEDIA_TYPES[extname(target).slice(1).toLowerCase()] ?? 'application/octet-stream',
        'Accept-Ranges': 'bytes',
      });
      const requested = request.headers.get('range');
      const range = requested === null ? undefined : byteRange(requested, size);
      if (requested !== null && range === undefined) {
        headers.set('Content-Range', `bytes */${size}`);
        headers.set('Content-Length', '0');
        await file.close();
        return new Response(null, { status: 416, headers });
      }
      const start = range?.start ?? 0;
      const end = range?.end ?? size - 1;
      headers.set('Content-Length', String(Math.max(0, end - start + 1)));
      if (range !== undefined) headers.set('Content-Range', `bytes ${start}-${end}/${size}`);
      if (size === 0 || request.method === 'HEAD') {
        await file.close();
        return new Response(null, { status: range === undefined ? 200 : 206, headers });
      }
      const body = Readable.toWeb(file.createReadStream({ start, end, autoClose: true }));
      return new Response(body as ReadableStream<Uint8Array>, {
        status: range === undefined ? 200 : 206,
        headers,
      });
    } catch (error) {
      await file.close();
      throw error;
    }
  }
}
