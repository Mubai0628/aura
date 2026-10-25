import { useCallback, useRef, useState } from 'react';

import {
  MAX_ATTACHED_BYTES,
  MAX_ATTACHMENTS,
  readAttachment,
  type ComposerAttachment,
  type PictureEncoder,
} from './attachment-files';

const carried = (attachment: ComposerAttachment) =>
  attachment.kind === 'file' ? 0 : attachment.bytes;

/**
 * The pictures and files added to the composer, from the + menu, a paste or a drop. A file
 * that cannot be sent is not added, and the composer says why.
 */
export function useComposerAttachments(encode?: PictureEncoder) {
  const [attachments, setAttachments] = useState<ComposerAttachment[]>([]);
  const [error, setError] = useState<string>();
  const current = useRef<ComposerAttachment[]>([]);
  const update = (next: ComposerAttachment[]) => {
    current.current = next;
    setAttachments(next);
  };
  const add = useCallback(
    async (files: readonly File[]) => {
      if (files.length === 0) return;
      setError(undefined);
      const place = (file: File) => window.aura.v1.attachmentPath?.(file) ?? '';
      const problems: string[] = [];
      let next = [...current.current];
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
    [encode],
  );
  const remove = (id: string) => {
    update(current.current.filter((attachment) => attachment.id !== id));
    setError(undefined);
  };
  const clear = () => {
    update([]);
    setError(undefined);
  };
  return { attachments, error, add, remove, clear };
}
