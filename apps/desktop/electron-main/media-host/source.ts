import { randomUUID } from 'node:crypto';
import { exact, record } from './protocol.js';
import { READ_BYTES, type MediaCommand, type MediaCore } from './types.js';

function rangeOf(text: string, size: number): { start: number; end: number } | undefined {
  const match = /^bytes=(\d*)-(\d*)$/iu.exec(text);
  if (!match || (!match[1] && !match[2])) return undefined;
  const first = match[1] ? Number(match[1]) : undefined;
  const last = match[2] ? Number(match[2]) : undefined;
  if (
    (first !== undefined && !Number.isSafeInteger(first)) ||
    (last !== undefined && !Number.isSafeInteger(last))
  )
    return undefined;
  if (first === undefined)
    return last && size > 0 ? { start: Math.max(0, size - last), end: size - 1 } : undefined;
  if (first >= size || (last !== undefined && last < first)) return undefined;
  return { start: first, end: Math.min(last ?? size - 1, size - 1) };
}

export function strictBase64(value: unknown, limit: number): Buffer {
  if (
    typeof value !== 'string' ||
    value.length > Math.ceil(limit / 3) * 4 ||
    value.length % 4 !== 0 ||
    !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u.test(value)
  )
    throw new Error('媒体读取返回了非法编码。');
  const bytes = Buffer.from(value, 'base64');
  if (bytes.length > limit || bytes.toString('base64') !== value)
    throw new Error('媒体读取返回了非法编码。');
  return bytes;
}

export class MediaSource {
  readonly url = `aura://media-decoder/${randomUUID()}/source`;
  #closed = false;
  constructor(
    private readonly command: MediaCommand,
    private readonly core: MediaCore,
    private readonly deadline: number,
  ) {}
  close(): void {
    this.#closed = true;
  }

  async container(): Promise<string | null> {
    const bytes = await this.#read(0, Math.min(4096, this.command.file_size));
    if (bytes.subarray(4, 8).toString('ascii') === 'ftyp') return 'mp4';
    if (bytes.subarray(0, 4).toString('ascii') === 'OggS') return 'ogg';
    if (bytes.subarray(0, 4).equals(Buffer.from([0x1a, 0x45, 0xdf, 0xa3]))) {
      const type = bytes.indexOf(Buffer.from([0x42, 0x82]));
      if (type >= 0 && bytes[type + 2] !== undefined) {
        const size = bytes[type + 2]! & 0x7f;
        const value = bytes.subarray(type + 3, type + 3 + size).toString('ascii');
        if (value === 'webm' || value === 'matroska') return value;
      }
      return 'matroska/webm';
    }
    return null;
  }

  async #read(offset: number, length: number): Promise<Buffer> {
    if (this.#closed || Date.now() >= this.deadline) throw new Error('媒体读取已结束或超时。');
    const { command_id, runtime_instance_id, session_id } = this.command;
    const result = await this.core.requestWithBudget(
      'media.host.read',
      {
        command_id,
        runtime_instance_id,
        session_id,
        offset,
        length,
      },
      Math.max(1, Math.min(15_000, this.deadline - Date.now())),
    );
    if (this.#closed) throw new Error('媒体读取已撤销。');
    if (!record(result) || !exact(result, 'data_base64')) throw new Error('媒体读取返回格式无效。');
    const bytes = strictBase64(result.data_base64, length);
    if (bytes.length !== length) throw new Error('媒体文件的读取长度发生变化，已停止解码。');
    return bytes;
  }

  async serve(request: Request): Promise<Response> {
    if (this.#closed || request.url !== this.url || !['GET', 'HEAD'].includes(request.method))
      return new Response('拒绝访问：不是本次活动媒体命令的地址。', { status: 403 });
    const size = this.command.file_size;
    const headers = new Headers({
      'Content-Type': this.command.media_type,
      'Accept-Ranges': 'bytes',
      'Cache-Control': 'no-store',
      'X-Content-Type-Options': 'nosniff',
      'Content-Security-Policy': "default-src 'none'; sandbox",
    });
    const asked = request.headers.get('range');
    const range = asked === null ? { start: 0, end: size - 1 } : rangeOf(asked, size);
    if (!range) {
      headers.set('Content-Range', `bytes */${size}`);
      headers.set('Content-Length', '0');
      return new Response(null, { status: 416, headers });
    }
    headers.set('Content-Length', String(range.end - range.start + 1));
    if (asked !== null) headers.set('Content-Range', `bytes ${range.start}-${range.end}/${size}`);
    let offset = range.start;
    let cancelled = false;
    const body =
      request.method === 'HEAD'
        ? null
        : new ReadableStream<Uint8Array>({
            pull: async (controller) => {
              try {
                if (offset > range.end) {
                  controller.close();
                  return;
                }
                const bytes = await this.#read(
                  offset,
                  Math.min(READ_BYTES, range.end - offset + 1),
                );
                if (cancelled) return;
                offset += bytes.length;
                controller.enqueue(new Uint8Array(bytes));
              } catch (error) {
                if (!cancelled) controller.error(error);
              }
            },
            cancel: () => {
              cancelled = true;
            },
          });
    return new Response(body, { status: asked === null ? 200 : 206, headers });
  }
}
