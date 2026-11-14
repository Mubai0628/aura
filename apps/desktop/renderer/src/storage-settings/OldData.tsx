import { useCallback, useEffect, useId, useState } from 'react';
import type { OldDataEntry, OldDataKind, OldDataRemoval } from '@aura/contracts/generated';

import { ConfirmationDialog } from '../ConfirmationDialog';
import { oldDataNotRemoved } from './storage-words';
import { storage } from './bridge';
import { size } from './size';

const KIND_NAMES: Record<OldDataKind, string> = {
  early_layout: '早期版本的数据',
  namespace: '旧安装的数据',
  unfinished: '没删完的旧数据',
};

/** What a row calls an item: a namespace's by the start of its folder name. */
function oldDataName(item: OldDataEntry): string {
  const short = item.kind === 'namespace' ? item.id.replace(/^namespace-/u, '').slice(0, 8) : '';
  return short ? `${KIND_NAMES[item.kind]} ${short}` : KIND_NAMES[item.kind];
}

/** The day of the newest change, in this computer's time. */
function day(timestamp: string): string {
  const date = new Date(timestamp);
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** What a removal did, one line each for what could not go. */
function outcome(removal: OldDataRemoval, items: OldDataEntry[]): string[] {
  const name = (id: string) => {
    const item = items.find((candidate) => candidate.id === id);
    return item ? oldDataName(item) : id;
  };
  return [
    ...removal.in_use.map((id) => `${name(id)} 正在使用，没有删`),
    ...removal.failed.map((failure) => `${name(failure.id)} 没删完：${failure.reason}`),
  ];
}

/**
 * 设置 → 系统 → 存储 → 旧数据: what older versions and other installations left and nothing uses,
 * each with its size, the day it last changed and its folders; the ticked ones go after one
 * confirmation. Absent while there is none.
 */
export function OldData({ coreReady }: { coreReady: boolean }) {
  const [items, setItems] = useState<OldDataEntry[]>();
  const [picked, setPicked] = useState<ReadonlySet<string>>(new Set());
  const [confirming, setConfirming] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [notes, setNotes] = useState<string[]>([]);
  const [failure, setFailure] = useState<string>();
  const titleId = useId();

  const show = useCallback((next: OldDataEntry[], ticked: boolean) => {
    setItems(next);
    setPicked(new Set(ticked ? next.map((item) => item.id) : []));
  }, []);

  useEffect(() => {
    const api = storage();
    if (!coreReady || api === undefined) return;
    let live = true;
    void api
      .oldData()
      .then((result) => {
        if (live && result.ok) show(result.value.items, true);
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [coreReady, show]);

  if (!items || items.length === 0) {
    return notes.length > 0 ? (
      <p className="settings-status-line" role="status">
        {notes.join('；')}
      </p>
    ) : null;
  }
  const chosen = items.filter((item) => picked.has(item.id));
  const total = chosen.reduce((sum, item) => sum + item.bytes, 0);

  const remove = async () => {
    const api = storage();
    if (api === undefined) return;
    setRemoving(true);
    try {
      const result = await api.removeOldData({ ids: chosen.map((item) => item.id) });
      if (result.ok) {
        setFailure(undefined);
        setNotes(outcome(result.value, items));
        show(result.value.items, false);
      } else {
        setFailure(oldDataNotRemoved(result.error));
      }
    } catch (reason) {
      setFailure(oldDataNotRemoved(reason));
    } finally {
      setRemoving(false);
      setConfirming(false);
    }
  };

  const toggle = (id: string, on: boolean) => {
    const next = new Set(picked);
    if (on) next.add(id);
    else next.delete(id);
    setPicked(next);
  };

  return (
    <section aria-label="旧数据" className="settings-card storage-old-data">
      <div className="settings-card-heading">
        <h3>旧数据</h3>
      </div>
      <ul className="storage-old-items">
        {items.map((item) => (
          <li key={item.id}>
            <label>
              <input
                checked={picked.has(item.id)}
                disabled={removing}
                onChange={(event) => toggle(item.id, event.target.checked)}
                type="checkbox"
              />
              <span>{oldDataName(item)}</span>
              <span className="storage-old-meta">
                {size(item.bytes)}
                {item.modified_at ? ` · ${day(item.modified_at)}` : ''}
              </span>
            </label>
            {item.paths.map((path) => (
              <p className="storage-path" key={path}>
                {path}
              </p>
            ))}
          </li>
        ))}
      </ul>
      {notes.length > 0 ? (
        <p className="settings-status-line" role="status">
          {notes.join('；')}
        </p>
      ) : null}
      {failure === undefined ? null : (
        <p className="settings-status-line" role="alert">
          {failure}
        </p>
      )}
      <div className="settings-card-actions">
        <button
          className="button is-small is-danger"
          disabled={removing || chosen.length === 0}
          onClick={() => setConfirming(true)}
          type="button"
        >
          删除所选
        </button>
      </div>
      {confirming ? (
        <ConfirmationDialog
          busy={removing}
          confirmLabel="删除"
          details={
            <ul className="storage-move-parts">
              {chosen.map((item) => (
                <li key={item.id}>
                  <span>{oldDataName(item)}</span>
                  <span>{size(item.bytes)}</span>
                </li>
              ))}
            </ul>
          }
          description={`共 ${size(total)}`}
          onCancel={() => setConfirming(false)}
          onConfirm={() => void remove()}
          title="删除所选的旧数据？"
          titleId={titleId}
        />
      ) : null}
    </section>
  );
}
