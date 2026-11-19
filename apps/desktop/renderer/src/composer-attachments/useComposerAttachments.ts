import { useCallback, useState } from 'react';

import { useKeptDraft } from '../agent-session/kept-draft';
import {
  MAX_ATTACHED_BYTES,
  MAX_ATTACHMENTS,
  readAttachment,
  type ComposerAttachment,
  type PictureEncoder,
} from './attachment-files';

const NONE: ComposerAttachment[] = [];
const COUNT_LIMIT = `一条消息最多附加 ${MAX_ATTACHMENTS} 个图片或文件。`;

const carried = (attachment: ComposerAttachment) =>
  attachment.kind === 'file' ? 0 : attachment.bytes;
const unreadable = (name: string) =>
  `「${name}」读不出来（可能已被移动、删除或正被其他程序占用），没有附加。确认文件能打开后再附加。`;

/** Attachments from the file picker, pasted images or a drop. */
export function useComposerAttachments(
  encode?: PictureEncoder,
  draftKey?: string,
  workspaceId?: string,
) {
  const saved = useKeptDraft('attachments', draftKey, NONE, workspaceId);
  const { value: attachments, begin, read: latest, update, clear } = saved;
  const [error, setError] = useState<string>();
  const add = useCallback(
    async (files: readonly File[]) => {
      if (files.length === 0) return;
      const batch = begin();
      setError(undefined);
      const problems: string[] = [];
      const place = (file: File) => window.aura.v1.attachmentPath?.(file) ?? '';
      try {
        if (batch.ready) await batch.ready;
        for (const file of files) {
          if (!batch.current()) return;
          if (latest().length >= MAX_ATTACHMENTS) {
            problems.push(COUNT_LIMIT);
            break;
          }
          let read: Awaited<ReturnType<typeof readAttachment>>;
          try {
            read = await readAttachment(file, place, encode);
          } catch {
            read = { error: unreadable(file.name || '文件') };
          }
          if (!batch.current()) return;
          if ('error' in read) {
            problems.push(read.error);
            continue;
          }
          const current = latest();
          if (current.length >= MAX_ATTACHMENTS) {
            problems.push(COUNT_LIMIT);
            break;
          }
          const total = current.reduce(
            (sum, attachment) => sum + carried(attachment),
            carried(read),
          );
          if (total > MAX_ATTACHED_BYTES) {
            problems.push(`「${read.name}」放不下了：一条消息的图片和文本文件合计最多 4 MB。`);
            continue;
          }
          update([...current, read]);
        }
        if (problems.length) setError([...new Set(problems)].join(' '));
      } finally {
        batch.finish();
      }
    },
    [begin, encode, latest, update],
  );
  return {
    attachments,
    importing: saved.loading || saved.pending > 0,
    error: error ?? saved.error,
    add,
    remove: (id: string) => {
      update((current) => current.filter((attachment) => attachment.id !== id));
      setError(undefined);
    },
    clear: () => {
      clear();
      setError(undefined);
    },
  };
}
