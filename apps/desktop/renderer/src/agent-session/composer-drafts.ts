import { useSyncExternalStore } from 'react';
import type { PermissionMode } from '@aura/contracts/generated';

const KEY = 'aura.composer-drafts.v1';
export interface ComposerDraft {
  id: string;
  text: string;
  needsReview: boolean;
  workspaceId?: string;
  serverSessionId?: string;
  modelId?: string;
  permissionMode?: PermissionMode;
}
const listeners = new Set<() => void>();
let records: ComposerDraft[] | undefined;
function read(): ComposerDraft[] {
  if (records) return records;
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    records = Array.isArray(value)
      ? value.filter(
          (item): item is ComposerDraft =>
            !!item &&
            typeof item === 'object' &&
            typeof item.id === 'string' &&
            typeof item.text === 'string' &&
            typeof item.needsReview === 'boolean' &&
            (item.workspaceId === undefined || typeof item.workspaceId === 'string') &&
            (item.serverSessionId === undefined || typeof item.serverSessionId === 'string') &&
            (item.modelId === undefined || typeof item.modelId === 'string') &&
            (item.permissionMode === undefined ||
              ['read_only', 'workspace_read_write', 'full_access'].includes(item.permissionMode)),
        )
      : [];
  } catch {
    records = [];
  }
  return records;
}
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
function publish(next: ComposerDraft[]): boolean {
  records = next;
  let saved = true;
  try {
    // An untouched local conversation has no durable record.
    localStorage.setItem(
      KEY,
      JSON.stringify(
        next.filter((item) => item.text.trim() || item.needsReview || item.serverSessionId),
      ),
    );
  } catch {
    saved = false;
  }
  for (const listener of listeners) listener();
  return saved;
}
export const getComposerDraft = (id: string) => read().find((item) => item.id === id);
export const useComposerDrafts = () => useSyncExternalStore(subscribe, read, read);
export function updateComposerDraft(
  id: string,
  patch: Partial<Omit<ComposerDraft, 'id'>>,
): boolean {
  const next = { text: '', needsReview: false, ...getComposerDraft(id), ...patch, id };
  return publish([...read().filter((item) => item.id !== id), next]);
}
export function createLocalConversation(workspaceId: string): string {
  const id = `draft_${crypto.randomUUID()}`;
  updateComposerDraft(id, { workspaceId });
  return id;
}
export function removeComposerDraft(id: string): void {
  publish(read().filter((item) => item.id !== id));
}
export function discardEmptyConversations(): void {
  publish(
    read().filter(
      (item) => !item.workspaceId || item.text.trim() || item.needsReview || item.serverSessionId,
    ),
  );
}
export function resetComposerDraftsForTests(): void {
  records = undefined;
}
