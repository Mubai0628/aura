import { binaryKind } from './file-kind';
import { mediaKind } from './file-paths';
import type { AttachmentFileTarget } from '../../../shared/attachment-files';
import { attachmentUnreadable, fileFailure } from './failure-words';
import { fileSource, outsideTarget } from './file-sources';

/** The lines one ranged read asks for: Core's small-range limit for files over 2 MiB. */
export const PAGE = 500;

export type FileContent =
  | { kind: 'text'; lines: string[]; total: number; paged: boolean; note?: string }
  | { kind: 'media'; media: 'image' | 'video' | 'audio'; src: string }
  | { kind: 'binary'; description: string; size: number; note?: string }
  | { kind: 'error'; message: string };

interface Where {
  session: string;
  workspace: string;
}

export { fileFailure } from './failure-words';

export function formatBytes(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}

const target = (where: Where, path: string) => ({
  session_id: where.session,
  workspace_id: where.workspace,
  target: { path },
});

/** A text's lines; the line break that ends a file does not start another line. */
function linesOf(text: string, whole: boolean): string[] {
  const lines = text.split(/\r\n|\n|\r/u);
  if (whole && lines.length > 1 && lines.at(-1) === '') lines.pop();
  return lines;
}

export async function readLines(where: Where, path: string, start: number) {
  const result = await window.aura.v1.readWorkspaceText({
    ...target(where, path),
    range: { range_kind: 'lines', start_line_index: start, count: PAGE },
    explicit_encoding: null,
  });
  return result.ok
    ? {
        ok: true as const,
        lines: linesOf(result.value.text, false),
        total: result.value.total_lines,
      }
    : { ok: false as const, message: fileFailure(result.error) };
}

async function describe(where: Where, path: string, note?: string): Promise<FileContent> {
  const info = await window.aura.v1.inspectWorkspaceFile(target(where, path));
  if (!info.ok) return { kind: 'error', message: fileFailure(info.error) };
  const description = binaryKind(path, info.value.media_type);
  return note === undefined
    ? { kind: 'binary', description, size: info.value.byte_length }
    : { kind: 'binary', description, size: info.value.byte_length, note };
}

/** A picture, a video or a sound streams from an address Main gives, whatever its size. */
async function readMedia(
  where: Where,
  path: string,
  media: 'image' | 'video' | 'audio',
): Promise<FileContent> {
  const given = await window.aura.v1.workspaceMediaUrl({
    session_id: where.session,
    workspace_id: where.workspace,
    path,
  });
  return given.ok
    ? { kind: 'media', media, src: given.url }
    : { kind: 'error', message: given.message };
}

/**
 * A file outside the workspace a message carried, through Main, which asks Core where that
 * one file is now: a picture, a video or a sound streams; text shows its beginning. An entry
 * of an attached zip, which Core copies out of it. An entry of a zip in the workspace, a
 * file outside the workspace a message names, a file the user picked — the same way.
 */
async function readOutside(target: AttachmentFileTarget, name: string): Promise<FileContent> {
  const media = mediaKind(name);
  if (media !== undefined) {
    const given = await window.aura.v1.attachmentFile({ ...target, action: 'media' });
    if (!given.ok) return { kind: 'error', message: given.message };
    return given.kind === 'media'
      ? { kind: 'media', media, src: given.url }
      : { kind: 'error', message: '没能打开这个附件，请重试。' };
  }
  const head = await window.aura.v1.attachmentFile({ ...target, action: 'text' });
  if (!head.ok) return { kind: 'error', message: head.message };
  if (head.kind === 'binary')
    return { kind: 'binary', description: binaryKind(name), size: head.size };
  if (head.kind !== 'text') return { kind: 'error', message: '没能打开这个附件，请重试。' };
  const lines = linesOf(head.text, !head.truncated);
  return head.truncated
    ? { kind: 'text', lines, total: lines.length, paged: false, note: '只显示了开头的 256 KB。' }
    : { kind: 'text', lines, total: lines.length, paged: false };
}

/** A text file or a zip's listing a message carried, from where the message keeps it. */
async function readRecord(where: Where, artifact: string): Promise<FileContent> {
  const read = await window.aura.v1.readAttachmentText({
    session_id: where.session,
    artifact_id: artifact,
  });
  if (!read.ok) return { kind: 'error', message: attachmentUnreadable(read.error) };
  const lines = linesOf(read.value.text, true);
  return { kind: 'text', lines, total: lines.length, paged: false };
}

/**
 * What a 文件 tab shows for a path. Text is read whole up to Core's 2 MiB limit and in
 * parts of 500 lines beyond it; a picture, a video or a sound streams from Main; anything
 * else that is not text is named by its type and size.
 */
export async function readFile(where: Where, path: string): Promise<FileContent> {
  try {
    const source = fileSource(path);
    const outside = outsideTarget(source, where.session);
    if (outside && source.kind !== 'workspace' && source.kind !== 'record')
      return await readOutside(outside, source.name);
    if (source.kind === 'record') return await readRecord(where, source.artifact);
    const media = mediaKind(path);
    if (media !== undefined) return await readMedia(where, path, media);
    const whole = await window.aura.v1.readWorkspaceText({
      ...target(where, path),
      range: { range_kind: 'full' },
      explicit_encoding: null,
    });
    if (whole.ok) {
      // Text with NUL characters is a binary file that happened to decode.
      if (whole.value.text.includes('\u0000')) return await describe(where, path);
      const lines = linesOf(whole.value.text, true);
      return { kind: 'text', lines, total: lines.length, paged: false };
    }
    if (whole.error.code === 'fs.range_required') {
      const first = await readLines(where, path, 0);
      return first.ok
        ? { kind: 'text', lines: first.lines, total: first.total, paged: true }
        : { kind: 'error', message: first.message };
    }
    if (whole.error.code === 'file.encoding_ambiguous') return await describe(where, path);
    return { kind: 'error', message: fileFailure(whole.error) };
  } catch (reason) {
    return { kind: 'error', message: fileFailure(reason) };
  }
}
