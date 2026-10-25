/**
 * A file outside
 * the workspace that a message of the conversation carried, for its 文件 tab and its menu. The
 * renderer names it by the conversation and the authorization the message gave; Main asks Core
 * where that one file is now (Core opens it and checks it is still the file attached), then
 * streams it, reads its beginning, or opens it outside Aura.
 */
export const ATTACHMENT_FILE_CHANNEL = 'aura:v1:attachment-file';

export type AttachmentFileAction = 'media' | 'text' | 'default_application' | 'file_manager';

/** The message that names a file, as the conversation shows it. */
export type MentionSource =
  | { kind: 'user'; event_id: string }
  | { kind: 'assistant'; model_step_id: string }
  | { kind: 'completion'; turn_task_id: string };

/**
 * The file asked about, one of four. A file outside the workspace a message carried, by its
 * authorization; or an entry of that zip. An entry of a zip in the conversation's
 * workspace; a file outside the workspace a message names (Core finds the path in that message
 * first); a file the user picked or dropped for the message (Main knows it from the preload, which
 * reads the place of the file the user chose).
 */
export type AttachmentFileTarget =
  | { session_id: string; external_grant_id: string; entry?: string }
  | { session_id: string; workspace_path: string; entry: string }
  | { session_id: string; mentioned_path: string; mentioned_in: MentionSource }
  | { picked_path: string };

export type AttachmentFileRequest = AttachmentFileTarget & { action: AttachmentFileAction };

/** The preload tells Main the place of a file the user picked or dropped. */
export const ATTACHMENT_PICKED_CHANNEL = 'aura:v1:attachment-picked';

/** The longest path of a zip entry or a file asked for (Core's own limit). */
const LONGEST = 1024;

/** The longest beginning of a text file shown. */
export const ATTACHMENT_TEXT_HEAD = 256 * 1024;

export type AttachmentFileResult =
  | { ok: true; kind: 'media'; url: string }
  | { ok: true; kind: 'text'; text: string; truncated: boolean; size: number }
  | { ok: true; kind: 'binary'; size: number }
  | { ok: true; kind: 'opened'; message: string }
  | { ok: false; message: string };

const ACTIONS: readonly string[] = ['media', 'text', 'default_application', 'file_manager'];
const ID = /^[a-z]{3}_[0-9A-HJKMNP-TV-Z]{26}$/u;
const NUL = String.fromCharCode(0);

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const id = (value: unknown, prefix: string): value is string =>
  typeof value === 'string' && ID.test(value) && value.startsWith(`${prefix}_`);
const text = (value: unknown): value is string =>
  typeof value === 'string' && value.length > 0 && value.length <= LONGEST && !value.includes(NUL);
/** A full Windows path. */
const absolute = (value: unknown): value is string => text(value) && /^[a-z]:[\\/]/iu.test(value);

function mention(value: unknown): MentionSource | undefined {
  if (!isRecord(value)) return undefined;
  const keys = Object.keys(value).sort().join(',');
  if (keys === 'event_id,kind' && value.kind === 'user' && id(value.event_id, 'evt'))
    return { kind: 'user', event_id: value.event_id };
  if (keys === 'kind,model_step_id' && value.kind === 'assistant' && id(value.model_step_id, 'stp'))
    return { kind: 'assistant', model_step_id: value.model_step_id };
  if (keys === 'kind,turn_task_id' && value.kind === 'completion' && id(value.turn_task_id, 'trn'))
    return { kind: 'completion', turn_task_id: value.turn_task_id };
  return undefined;
}

function target(value: Record<string, unknown>, keys: string): AttachmentFileTarget | undefined {
  const session = value.session_id;
  switch (keys) {
    case 'external_grant_id,session_id':
    case 'entry,external_grant_id,session_id':
      if (!id(session, 'ses') || !id(value.external_grant_id, 'xgr')) return undefined;
      if ('entry' in value && !text(value.entry)) return undefined;
      return {
        session_id: session,
        external_grant_id: value.external_grant_id,
        ...(typeof value.entry === 'string' ? { entry: value.entry } : {}),
      };
    case 'entry,session_id,workspace_path':
      return id(session, 'ses') && text(value.workspace_path) && text(value.entry)
        ? { session_id: session, workspace_path: value.workspace_path, entry: value.entry }
        : undefined;
    case 'mentioned_in,mentioned_path,session_id': {
      const source = mention(value.mentioned_in);
      return id(session, 'ses') && absolute(value.mentioned_path) && source
        ? { session_id: session, mentioned_path: value.mentioned_path, mentioned_in: source }
        : undefined;
    }
    case 'picked_path':
      return absolute(value.picked_path) ? { picked_path: value.picked_path } : undefined;
    default:
      return undefined;
  }
}

/** Main takes the request only in one of its exact shapes; the renderer is not trusted. */
export function parseAttachmentFileRequest(value: unknown): AttachmentFileRequest | undefined {
  if (!isRecord(value) || typeof value.action !== 'string' || !ACTIONS.includes(value.action))
    return undefined;
  const keys = Object.keys(value)
    .filter((key) => key !== 'action')
    .sort()
    .join(',');
  const found = target(value, keys);
  return found ? { ...found, action: value.action as AttachmentFileAction } : undefined;
}

/** What the renderer reads back. */
export function readAttachmentFileResult(value: unknown): AttachmentFileResult {
  if (isRecord(value) && value.ok === true) {
    if (
      value.kind === 'media' &&
      typeof value.url === 'string' &&
      value.url.startsWith('aura://app/workspace-media/')
    )
      return { ok: true, kind: 'media', url: value.url };
    if (
      value.kind === 'text' &&
      typeof value.text === 'string' &&
      typeof value.truncated === 'boolean' &&
      typeof value.size === 'number'
    )
      return {
        ok: true,
        kind: 'text',
        text: value.text,
        truncated: value.truncated,
        size: value.size,
      };
    if (value.kind === 'binary' && typeof value.size === 'number')
      return { ok: true, kind: 'binary', size: value.size };
    if (value.kind === 'opened' && typeof value.message === 'string')
      return { ok: true, kind: 'opened', message: value.message };
  }
  if (isRecord(value) && value.ok === false && typeof value.message === 'string')
    return { ok: false, message: value.message };
  return { ok: false, message: '没能打开这个附件，请重试。' };
}
