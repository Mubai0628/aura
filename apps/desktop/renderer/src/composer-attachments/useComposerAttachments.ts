import { useCallback, useRef, useState } from 'react';

import { useKeptDraft } from '../agent-session/kept-draft';
import {
  MAX_ATTACHED_BYTES,
  MAX_ATTACHMENTS,
  readAttachment,
  type ComposerAttachment,
  type PictureEncoder,
} from './attachment-files';

const NONE: ComposerAttachment[] = [];

const carried = (attachment: ComposerAttachment) =>
  attachment.kind === 'file' ? 0 : attachment.bytes;

/**
 * The pictures and files added to the composer, from the + menu, a paste or a drop, kept for the
 * conversation `draftKey` names while Aura runs. A file that cannot be sent is not added, and the
 * composer says why.
 */
export function useComposerAttachments(encode?: PictureEncoder, draftKey?: string) {
  const [attachments, setAttachments] = useKeptDraft('attachments', draftKey, NONE);
  const [error, setError] = useState<string>();
  // The list as last changed here, for the conversation it was changed for, so changes made
  // while an add awaits its files build on each other.
  const changed = useRef<{ key: string | undefined; list: ComposerAttachment[] }>(undefined);
  const latest = useCallback(() => {
    const last = changed.current;
    return last !== undefined && last.key === draftKey ? last.list : attachments;
  }, [attachments, draftKey]);
  const update = useCallback(
    (next: ComposerAttachment[]) => {
      changed.current = { key: draftKey, list: next };
      setAttachments(next);
    },
    [draftKey, setAttachments],
  );
  const add = useCallback(
    async (files: readonly File[]) => {
      if (files.length === 0) return;
      setError(undefined);
      const place = (file: File) => window.aura.v1.attachmentPath?.(file) ?? '';
      const problems: string[] = [];
      let next = [...latest()];
      for (const file of files) {
        if (next.length >= MAX_ATTACHMENTS) {
          problems.push(`一条消息最多附加 ${MAX_ATTACHMENTS} 个图片或文件。`);
          break;
        }
        const read = await readAttachment(file, place, encode);
        if ('error' in read) {
          problems.push(read.error);
          continue;
        }
        const total = next.reduce((sum, attachment) => sum + carried(attachment), carried(read));
        if (total > MAX_ATTACHED_BYTES) {
          problems.push(`「${read.name}」放不下了：一条消息的图片和文本文件合计最多 4 MB。`);
          continue;
        }
        next = [...next, read];
      }
      update(next);
      if (problems.length) setError(problems.join(' '));
    },
    [encode, latest, update],
  );
  const remove = (id: string) => {
    update(latest().filter((attachment) => attachment.id !== id));
    setError(undefined);
  };
  const clear = () => {
    update([]);
    setError(undefined);
  };
  return { attachments, error, add, remove, clear };
}
