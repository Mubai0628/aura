import type { AgentMessageAttachment } from '@aura/contracts/generated';
import { workspaceZipEntry, zipEntry } from './file-sources';

/**
 * A file inside a zip
 * a message of the conversation carried from outside the workspace, named by a link in the
 * conversation. The zip's listing, kept with its message, says what is in it; a link names an
 * entry by its whole path in the zip, or by its last parts.
 */

/**
 * A zip a message carried, and its listing: from outside the workspace, by the authorization to
 * read it; or in the workspace, by its path there.
 */
export type CarriedZip = { listing: string } & ({ grant: string } | { workspace: string });

/** The zips the messages carried, the latest last. */
export function carriedZips(
  messages: readonly ({ attachments?: AgentMessageAttachment[] | null } | undefined)[],
): CarriedZip[] {
  return messages.flatMap((message) =>
    (message?.attachments ?? []).flatMap((attachment): CarriedZip[] => {
      if (attachment.kind === 'external_file' && attachment.listing_artifact_id)
        return [{ grant: attachment.external_grant_id, listing: attachment.listing_artifact_id }];
      if (attachment.kind === 'file' && attachment.listing_artifact_id)
        return [{ workspace: attachment.path, listing: attachment.listing_artifact_id }];
      return [];
    }),
  );
}

/** A line of a listing: the entry's path, its size, and whether it is a folder or encrypted. */
const LINE = /^- (.+) — \d+ B((?: \[目录\]| \[加密\])*)$/u;

/** The files a listing names: not its folders, nor what is encrypted. */
export function listedFiles(listing: string): string[] {
  return listing.split('\n').flatMap((line) => {
    const found = LINE.exec(line.trimEnd());
    return found && !found[2] ? [found[1]!] : [];
  });
}

/** The entry a link names: its whole path, else the one file ending in it nearest the root. */
export function entryNamed(files: readonly string[], link: string): string | undefined {
  const wanted = link.replaceAll('\\', '/').replace(/^(?:\.\/)+/u, '');
  if (!wanted) return undefined;
  if (files.includes(wanted)) return wanted;
  const ending = files.filter((file) => file.endsWith(`/${wanted}`));
  if (ending.length <= 1) return ending[0];
  const depth = (file: string) => file.split('/').length;
  const nearest = Math.min(...ending.map(depth));
  const top = ending.filter((file) => depth(file) === nearest);
  return top.length === 1 ? top[0] : undefined;
}

/** A message's listing never changes: each is read once, and read again only after a failure. */
const listings = new Map<string, Promise<string[] | undefined>>();

function filesOf(session: string, zip: CarriedZip): Promise<string[] | undefined> {
  const key = `${session}:${zip.listing}`;
  let read = listings.get(key);
  if (read === undefined) {
    read = window.aura.v1
      .readAttachmentText({ session_id: session, artifact_id: zip.listing })
      .then((result) => (result.ok ? listedFiles(result.value.text) : undefined))
      .catch(() => undefined);
    listings.set(key, read);
    void read.then((files) => {
      if (files === undefined) listings.delete(key);
    });
  }
  return read;
}

/** Finds a link's entry in the conversation's zips, the latest first. */
export function zipEntryFinder(
  zips: readonly CarriedZip[],
  session: string,
): ((link: string) => Promise<string | undefined>) | undefined {
  if (!zips.length) return undefined;
  const files = async (zip: CarriedZip) => (await filesOf(session, zip)) ?? [];
  return async (link) => {
    for (const zip of [...zips].reverse()) {
      const entry = entryNamed(await files(zip), link);
      if (entry !== undefined)
        return 'grant' in zip
          ? zipEntry(zip.grant, entry)
          : workspaceZipEntry(zip.workspace, entry);
    }
    return undefined;
  };
}
