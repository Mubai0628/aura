import { useEffect, useMemo, useSyncExternalStore } from 'react';
import {
  beginComposerDraftWork,
  getComposerDraft,
  setDraftExtraPresent,
  type ExtraDraftKind,
} from './composer-drafts';
import { readDraftExtra, saveDraftExtra } from './draft-extra-storage';

type Items = readonly unknown[];
type Update<T> = T | ((old: T) => T);
type Snapshot<T> = { value: T; loading: boolean; pending: number; error?: string | undefined };
const kept = new Map<string, DraftCell<Items>>();
const EMPTY: Items = [];
const SAVE_ERROR = '草稿引用暂时无法保存到本机，请保留原内容。';
const LOAD_ERROR = '未能恢复草稿引用。可以先查看原文件，再重新附加。';

class DraftCell<T extends Items> {
  private readonly listeners = new Set<() => void>();
  private snapshot: Snapshot<T>;
  private queued: Update<T>[] = [];
  private hydration?: Promise<void>;
  private revision = 0;
  private epoch = 0;

  constructor(
    private readonly kind: ExtraDraftKind,
    private readonly key: string | undefined,
    private readonly empty: T,
  ) {
    this.snapshot = {
      value: empty,
      loading: key !== undefined && Boolean(getComposerDraft(key)?.extraKinds?.includes(kind)),
      pending: 0,
    };
  }

  read = () => this.snapshot;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  private notify(next: Partial<Snapshot<T>>) {
    this.snapshot = { ...this.snapshot, ...next };
    for (const listener of this.listeners) listener();
  }

  private get storageKey() {
    return `${this.kind}:${this.key}`;
  }

  hydrate(workspaceId?: string): Promise<void> | undefined {
    if (!this.snapshot.loading || this.key === undefined) return this.hydration;
    this.hydration ??= readDraftExtra(this.storageKey)
      .then((stored) => {
        const next = this.withQueued((stored as T | undefined) ?? this.empty);
        const dirty = this.queued.length > 0;
        this.queued = [];
        this.notify({ loading: false, value: next });
        if (dirty || !stored) this.persist(next, workspaceId);
      })
      .catch(() => {
        const next = this.withQueued(this.empty);
        this.queued = [];
        this.notify({ loading: false, value: next, error: LOAD_ERROR });
      });
    return this.hydration;
  }

  private withQueued(value: T) {
    return this.queued.reduce<T>((old, change) => this.apply(old, change), value);
  }

  private apply(value: T, change: Update<T>): T {
    return typeof change === 'function' ? (change as (value: T) => T)(value) : change;
  }

  update(change: Update<T>, workspaceId?: string) {
    if (this.snapshot.loading) {
      this.queued.push(change);
      void this.hydrate(workspaceId);
      return;
    }
    const next = this.apply(this.snapshot.value, change);
    if (next === this.snapshot.value && !this.snapshot.error) return;
    this.notify({ value: next, error: undefined });
    this.persist(next, workspaceId);
  }

  private persist(value: T, workspaceId?: string) {
    const id = this.key;
    if (id === undefined) return;
    const revision = ++this.revision;
    if (!setDraftExtraPresent(id, this.kind, value.length > 0, workspaceId))
      this.notify({ error: SAVE_ERROR });
    if (typeof indexedDB === 'undefined') return;
    void saveDraftExtra(this.storageKey, value).catch(() => {
      if (this.revision === revision) this.notify({ error: SAVE_ERROR });
    });
  }

  begin(workspaceId?: string) {
    const epoch = this.epoch;
    const done =
      this.key === undefined
        ? undefined
        : beginComposerDraftWork(this.key, this.key.startsWith('draft_') ? workspaceId : undefined);
    this.notify({ pending: this.snapshot.pending + 1 });
    return {
      ready: this.hydrate(workspaceId),
      current: () => this.epoch === epoch,
      finish: () => {
        done?.();
        if (this.epoch === epoch) this.notify({ pending: this.snapshot.pending - 1 });
      },
    };
  }

  clear(workspaceId?: string) {
    this.epoch += 1;
    this.update(this.empty, workspaceId);
    this.notify({ pending: 0 });
  }
}

function cell<T extends Items>(kind: ExtraDraftKind, key: string | undefined, empty: T) {
  if (key === undefined) return new DraftCell(kind, key, empty);
  const id = `${kind}:${key}`;
  const existing = kept.get(id);
  if (existing) return existing as unknown as DraftCell<T>;
  const entry = new DraftCell(kind, key, empty);
  kept.set(id, entry as unknown as DraftCell<Items>);
  return entry;
}

export function useKeptDraft<T extends Items>(
  kind: ExtraDraftKind,
  key: string | undefined,
  empty: T,
  workspaceId?: string,
) {
  const entry = useMemo(() => cell(kind, key, empty), [kind, key, empty]);
  const state = useSyncExternalStore(entry.subscribe, entry.read, entry.read);
  useEffect(() => void entry.hydrate(workspaceId), [entry, workspaceId]);
  const actions = useMemo(
    () => ({
      update: (next: Update<T>) => entry.update(next, workspaceId),
      read: () => entry.read().value,
      begin: () => entry.begin(workspaceId),
      clear: () => entry.clear(workspaceId),
    }),
    [entry, workspaceId],
  );
  return { ...state, ...actions };
}

export async function moveKeptDrafts(from: string, to: string) {
  for (const kind of getComposerDraft(from)?.extraKinds ?? []) {
    const source = cell(kind, from, EMPTY);
    await source.hydrate();
    cell(kind, to, EMPTY).update(source.read().value);
    source.clear();
  }
}

export function resetKeptDraftsForTests(): void {
  kept.clear();
}
