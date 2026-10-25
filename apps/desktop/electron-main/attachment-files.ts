import { open, stat } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import { ipcMain, shell, type IpcMainEvent, type IpcMainInvokeEvent } from 'electron';
import {
  ATTACHMENT_FILE_CHANNEL,
  ATTACHMENT_PICKED_CHANNEL,
  ATTACHMENT_TEXT_HEAD,
  parseAttachmentFileRequest,
  type AttachmentFileRequest,
  type AttachmentFileResult,
} from '../shared/attachment-files.js';
import { runsCode } from '../shared/outside-files.js';
import { WORKSPACE_MEDIA } from '../shared/workspace-media.js';
import type { CoreSupervisor } from './core-supervisor.js';
import type { WorkspaceMedia } from './workspace-media.js';

type Core = Pick<CoreSupervisor, 'requestWithBudget'>;

const MISSING = '找不到这个附件，它可能已被移动、删除或替换。';

/** A zip's entry that did not come out, for a reason Core did not say. */
const ENTRY_UNFINISHED = '没能从压缩包里取出这个文件，请稍后重试。';

/** Core's sentence for the user, without its code; anything else is said plainly. */
function reason(error: unknown, entry = false): string {
  const text = error instanceof Error ? error.message : String(error);
  const said = /agent\.(?:attachment|mentioned_file)_[a-z_]+:\s*(.+)$/u.exec(text);
  return said?.[1]?.trim() || (entry ? ENTRY_UNFINISHED : MISSING);
}

/** How many files the user picked are remembered for viewing. */
const PICKED_KEPT = 500;
const fold = (path: string) => path.replaceAll('/', '\\').toLowerCase();

function location(value: unknown): { path: string; size: number } | undefined {
  if (typeof value !== 'object' || value === null) return undefined;
  const { path, byte_length: size } = value as Record<string, unknown>;
  return typeof path === 'string' && path.length > 0 && typeof size === 'number'
    ? { path, size }
    : undefined;
}

/** The beginning of a text file, decoded leniently; a file with NUL characters is not text. */
async function textHead(path: string, size: number): Promise<AttachmentFileResult> {
  const file = await open(path, 'r');
  try {
    const length = Math.min(size, ATTACHMENT_TEXT_HEAD);
    const buffer = Buffer.alloc(length);
    const { bytesRead } = await file.read(buffer, 0, length, 0);
    const text = new TextDecoder('utf-8').decode(buffer.subarray(0, bytesRead));
    if (text.includes('\u0000')) return { ok: true, kind: 'binary', size };
    return { ok: true, kind: 'text', text, truncated: size > bytesRead, size };
  } finally {
    await file.close();
  }
}

/**
 * A file outside
 * the workspace that a message carried. Main never takes a path from the renderer: it asks Core,
 * which opens the file through the conversation's read-only authorization for that one file and
 * says where it is now. Pictures, video and sound stream from an address of Aura's own scheme;
 * text shows its beginning; programs and scripts are never opened, only shown in the file manager.
 */
export class AttachmentFiles {
  /** The places of the files the user picked or dropped, as the preload read them. */
  readonly #picked = new Set<string>();

  constructor(private readonly media: WorkspaceMedia) {}

  install(
    core: () => Core | undefined,
    assertTrustedSender: (event: IpcMainInvokeEvent | IpcMainEvent) => void,
  ) {
    ipcMain.handle(ATTACHMENT_FILE_CHANNEL, async (event, value: unknown) => {
      assertTrustedSender(event);
      return this.handle(core(), value);
    });
    ipcMain.on(ATTACHMENT_PICKED_CHANNEL, (event, value: unknown) => {
      assertTrustedSender(event);
      this.picked(value);
    });
  }

  /** A file the user picked or dropped: from now on it can be viewed by its place. */
  picked(value: unknown): void {
    if (typeof value !== 'string' || value.length > 1024 || !isAbsolute(value)) return;
    const key = fold(value);
    this.#picked.delete(key);
    this.#picked.add(key);
    if (this.#picked.size > PICKED_KEPT) {
      const oldest = this.#picked.values().next().value;
      if (oldest !== undefined) this.#picked.delete(oldest);
    }
  }

  async handle(core: Core | undefined, value: unknown): Promise<AttachmentFileResult> {
    const request = parseAttachmentFileRequest(value);
    if (request === undefined) return { ok: false, message: '无效的附件请求' };
    let found: { path: string; size: number } | undefined;
    try {
      found = await this.#locate(core, request);
    } catch (error) {
      return { ok: false, message: reason(error, 'entry' in request) };
    }
    if (found === undefined) return { ok: false, message: MISSING };
    const { path, size } = found;
    try {
      switch (request.action) {
        case 'media':
          if (!WORKSPACE_MEDIA.test(path) || !(await stat(path)).isFile())
            return { ok: false, message: '这个文件不是图片、视频或音频' };
          return { ok: true, kind: 'media', url: this.media.register(path) };
        case 'text':
          return await textHead(path, size);
        case 'file_manager':
          shell.showItemInFolder(path);
          return { ok: true, kind: 'opened', message: '已在文件资源管理器中显示' };
        case 'default_application': {
          if (runsCode(path))
            return {
              ok: false,
              message: '这是可执行文件或脚本，Aura 不会直接运行它；可以在文件资源管理器中查看。',
            };
          const failure = await shell.openPath(path);
          return failure
            ? { ok: false, message: `没有打开这个文件：${failure}` }
            : { ok: true, kind: 'opened', message: '已用默认应用打开' };
        }
      }
    } catch {
      return { ok: false, message: MISSING };
    }
  }

  /**
   * Where the file asked about is now. Core says so for what a message carried or names; a zip's
   * entry is Core's copy of it, made within the longest budget Main gives a request (Core keeps
   * its entries small enough for it). A file the user picked is where the preload saw it.
   */
  async #locate(
    core: Core | undefined,
    request: AttachmentFileRequest,
  ): Promise<{ path: string; size: number } | undefined> {
    if ('picked_path' in request) {
      if (!this.#picked.has(fold(request.picked_path)))
        throw new Error('agent.attachment_unpicked: 请重新选择这个文件。');
      const found = await stat(request.picked_path).catch(() => undefined);
      return found?.isFile() ? { path: request.picked_path, size: found.size } : undefined;
    }
    if (core === undefined)
      throw new Error('agent.attachment_core: Aura Core 尚未就绪，请稍后再试');
    const ask = (method: string, params: unknown) => core.requestWithBudget(method, params, 15_000);
    if ('mentioned_path' in request)
      return location(
        await ask('agent.files.mentioned', {
          session_id: request.session_id,
          path: request.mentioned_path,
          mentioned_in: request.mentioned_in,
        }),
      );
    if ('workspace_path' in request)
      return location(
        await ask('agent.attachments.entry', {
          session_id: request.session_id,
          workspace_path: request.workspace_path,
          entry: request.entry,
        }),
      );
    const attached = {
      session_id: request.session_id,
      external_grant_id: request.external_grant_id,
    };
    return location(
      request.entry === undefined
        ? await ask('agent.attachments.locate', attached)
        : await ask('agent.attachments.entry', { ...attached, entry: request.entry }),
    );
  }
}
