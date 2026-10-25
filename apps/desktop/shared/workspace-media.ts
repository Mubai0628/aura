/**
 * A picture, video or sound of a
 * conversation's workspace, shown in its 文件 tab. The renderer names the file by its workspace
 * path; Main finds it inside the workspace and gives back an address of Aura's own scheme that
 * streams it, ranges and all, to the tab's player.
 */
export const WORKSPACE_MEDIA_CHANNEL = 'aura:v1:workspace-media';

export interface WorkspaceMediaRequest {
  session_id: string;
  workspace_id: string;
  /** Workspace-relative, with `/` between parts. */
  path: string;
}

export type WorkspaceMediaResult = { ok: true; url: string } | { ok: false; message: string };

/** The kinds Main streams: what an `<img>`, `<video>` or `<audio>` element shows. */
export const WORKSPACE_MEDIA =
  /\.(?:png|jpe?g|gif|webp|bmp|ico|avif|svg|mp4|m4v|webm|ogv|mov|mp3|wav|flac|ogg|oga|m4a|aac|opus)$/iu;

const ID = /^[a-z]{3}_[0-9A-HJKMNP-TV-Z]{26}$/u;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

/** Main takes the request only in its exact shape; the renderer is not trusted. */
export function parseWorkspaceMediaRequest(value: unknown): WorkspaceMediaRequest | undefined {
  if (!isRecord(value)) return undefined;
  const keys = Object.keys(value).sort();
  if (keys.join(',') !== 'path,session_id,workspace_id') return undefined;
  const { session_id, workspace_id, path } = value;
  return typeof session_id === 'string' &&
    typeof workspace_id === 'string' &&
    typeof path === 'string' &&
    ID.test(session_id) &&
    ID.test(workspace_id) &&
    path.length <= 1024
    ? { session_id, workspace_id, path }
    : undefined;
}

/** What the renderer reads back: an address of Aura's own scheme, or why not. */
export function readWorkspaceMediaResult(value: unknown): WorkspaceMediaResult {
  if (isRecord(value) && value.ok === true && typeof value.url === 'string') {
    if (value.url.startsWith('aura://app/workspace-media/')) return { ok: true, url: value.url };
  }
  if (isRecord(value) && value.ok === false && typeof value.message === 'string')
    return { ok: false, message: value.message };
  return { ok: false, message: '没能打开这个文件，请重试。' };
}
