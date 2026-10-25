import type { AttachmentFileTarget, MentionSource } from '../../../shared/attachment-files';

/**
 * What a 文件
 * tab shows. Most entries are workspace files, by their path; a message also carries files the
 * tab shows: a file outside the workspace, by the authorization the message gave the
 * conversation, and a text file or a zip's listing, kept with the message. They are written into
 * the tab's list of paths with a prefix no workspace path has (`:` is never in a Windows name).
 * An entry of a zip in the workspace, a file outside the workspace a message names, and a
 * file the user picked for a message are shown too.
 */
export type FileSource =
  | { kind: 'workspace'; path: string }
  /** `entry`, a file inside the attached zip, by its path in the zip. */
  | { kind: 'attachment'; grant: string; name: string; entry?: string }
  | { kind: 'record'; artifact: string; name: string }
  /** An entry of a zip in the workspace. */
  | { kind: 'zipped'; zip: string; entry: string; name: string }
  /** A file outside the workspace that a message names by its full path. */
  | { kind: 'mentioned'; path: string; from: MentionSource; name: string }
  /** A file the user picked or dropped for a message. */
  | { kind: 'picked'; path: string; name: string };

const ID = '[a-z]{3}_[0-9A-HJKMNP-TV-Z]{26}';
const ATTACHMENT = new RegExp(`^attachment:(xgr_[0-9A-HJKMNP-TV-Z]{26})/([^/\\\\]+)$`, 'u');
const ZIPPED = /^zipentry:(xgr_[0-9A-HJKMNP-TV-Z]{26})\/(.+)$/u;
const RECORD = new RegExp(`^record:(art_[0-9A-HJKMNP-TV-Z]{26})/([^/\\\\]+)$`, 'u');
const WORKSPACE_ZIPPED = /^wszip:([^|]+)\|(.+)$/u;
const MENTIONED = new RegExp(`^mentioned:(user|assistant|completion):(${ID})\\|(.+)$`, 'u');
const PICKED = /^picked:(.+)$/u;

/** The last part of a path, with either slash. */
const lastPart = (path: string) => path.split(/[\\/]/u).at(-1) ?? path;

/** A file outside the workspace a message carried. */
export const attachmentEntry = (grant: string, name: string) => `attachment:${grant}/${name}`;
/** A text file or a zip's listing a message carried. */
export const recordEntry = (artifact: string, name: string) => `record:${artifact}/${name}`;
/** A file inside a zip outside the workspace a message carried. */
export const zipEntry = (grant: string, entry: string) => `zipentry:${grant}/${entry}`;
/** A file inside a zip in the workspace. */
export const workspaceZipEntry = (zip: string, entry: string) => `wszip:${zip}|${entry}`;
/** A file outside the workspace that a message names. */
export function mentionedEntry(from: MentionSource, path: string): string {
  const id =
    from.kind === 'user'
      ? from.event_id
      : from.kind === 'assistant'
        ? from.model_step_id
        : from.turn_task_id;
  return `mentioned:${from.kind}:${id}|${path}`;
}
/** A file the user picked or dropped for a message. */
export const pickedEntry = (path: string) => `picked:${path}`;

function mention(kind: string, id: string): MentionSource {
  if (kind === 'user') return { kind, event_id: id };
  if (kind === 'assistant') return { kind, model_step_id: id };
  return { kind: 'completion', turn_task_id: id };
}

export function fileSource(entry: string): FileSource {
  const attached = ATTACHMENT.exec(entry);
  if (attached) return { kind: 'attachment', grant: attached[1]!, name: attached[2]! };
  const zipped = ZIPPED.exec(entry);
  if (zipped) {
    const inside = zipped[2]!;
    return { kind: 'attachment', grant: zipped[1]!, name: lastPart(inside), entry: inside };
  }
  const kept = RECORD.exec(entry);
  if (kept) return { kind: 'record', artifact: kept[1]!, name: kept[2]! };
  const local = WORKSPACE_ZIPPED.exec(entry);
  if (local) return { kind: 'zipped', zip: local[1]!, entry: local[2]!, name: lastPart(local[2]!) };
  const named = MENTIONED.exec(entry);
  if (named)
    return {
      kind: 'mentioned',
      from: mention(named[1]!, named[2]!),
      path: named[3]!,
      name: lastPart(named[3]!),
    };
  const picked = PICKED.exec(entry);
  if (picked) return { kind: 'picked', path: picked[1]!, name: lastPart(picked[1]!) };
  return { kind: 'workspace', path: entry };
}

/**
 * What Main is asked about for a file the 文件 tab shows from outside the workspace's own files:
 * one a message carried, an entry of a zip, one a message names, one the user picked. Nothing
 * for a workspace file or a kept text.
 */
export function outsideTarget(
  source: FileSource,
  session: string,
): AttachmentFileTarget | undefined {
  switch (source.kind) {
    case 'attachment':
      return {
        session_id: session,
        external_grant_id: source.grant,
        ...(source.entry === undefined ? {} : { entry: source.entry }),
      };
    case 'zipped':
      return { session_id: session, workspace_path: source.zip, entry: source.entry };
    case 'mentioned':
      return { session_id: session, mentioned_path: source.path, mentioned_in: source.from };
    case 'picked':
      return { picked_path: source.path };
    default:
      return undefined;
  }
}

/** Whether the entry is a workspace file (the tree, the file manager and paths apply). */
export const inWorkspace = (entry: string) => fileSource(entry).kind === 'workspace';

/** The name an entry is shown by. */
export function entryName(entry: string): string {
  const source = fileSource(entry);
  return source.kind === 'workspace' ? lastPart(source.path) : source.name;
}
