import type { MessageReferenceView } from '@aura/contracts/generated';

/** How a reference reads beside its message: a folder ends with "/", the root is named. */
export function referenceText(reference: MessageReferenceView): string {
  if (reference.kind === 'session') return `会话「${reference.label}」`;
  if (reference.kind === 'folder')
    return reference.label === '.' ? '工作区根目录' : `${reference.label}/`;
  return reference.label;
}
