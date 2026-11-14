import type { WorktreeLocationView } from '@aura/contracts/generated';
import { useEffect, useState } from 'react';
import { size } from './size';
import { refusedAction } from '../agent-session/failure-text';
import { worktreeNotMoved } from './storage-words';

type Read = { view: WorktreeLocationView } | { problem: string };

/** The location was not read: why, and that it cannot be changed until it is. */
const unread = (error: unknown): string =>
  refusedAction(error, '工作副本的位置没有读到', '暂时不能更改；请点“重试”。');

const readLocation = async (): Promise<Read> => {
  try {
    const result = await window.aura.v1.readWorktreeLocation();
    return result.ok ? { view: result.value } : { problem: unread(result.error) };
  } catch (reason) {
    return { problem: unread(reason) };
  }
};

/**
 * 设置 → 存储 → 工作副本: the folder new worktrees go under, how many there are and what they
 * take on disk, the drive's free space; choosing another folder and going back to the default.
 * Worktrees already made stay where they are.
 */
export function WorktreeLocationRow() {
  const [view, setView] = useState<WorktreeLocationView>();
  const [problem, setProblem] = useState<string>();
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let live = true;
    void readLocation().then((read) => {
      if (!live) return;
      if ('view' in read) setView(read.view);
      else setProblem(read.problem);
    });
    return () => {
      live = false;
    };
  }, []);
  const save = async (location: string | null) => {
    if (view === undefined) return;
    const result = await window.aura.v1.setWorktreeLocation({
      expected_revision: view.revision,
      ...(location === null ? {} : { location }),
    });
    if (result.ok) {
      setView(result.value);
      setProblem(undefined);
      return;
    }
    // The reason stays on screen; the folder shown is read again.
    setProblem(worktreeNotMoved(result.error));
    const again = await readLocation();
    if ('view' in again) setView(again.view);
  };
  const act = async (work: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    try {
      await work();
    } catch (reason) {
      setProblem(worktreeNotMoved(reason));
    } finally {
      setBusy(false);
    }
  };
  const retry = async () => {
    const read = await readLocation();
    if ('view' in read) {
      setView(read.view);
      setProblem(undefined);
    } else setProblem(read.problem);
  };
  const choose = () =>
    act(async () => {
      const picked = await window.aura.v1.selectWorktreeLocation();
      if (!picked.ok)
        setProblem(refusedAction(picked.error, '没有打开选择框', '工作副本位置没有改；请重试。'));
      else if (picked.value.path) await save(picked.value.path);
    });
  return (
    <section aria-label="工作副本" className="settings-card storage-place">
      <div className="settings-card-heading">
        <h3>工作副本</h3>
        {view?.size_bytes != null ? (
          <span className="settings-tag">{size(view.size_bytes)}</span>
        ) : null}
      </div>
      {view ? (
        <>
          <p className="storage-path">{view.location}</p>
          <p className="storage-worktree-facts">
            {view.worktrees} 个{view.free_bytes != null ? ` · 剩余 ${size(view.free_bytes)}` : ''}
          </p>
        </>
      ) : null}
      {problem ? (
        <p className="settings-status-line" role="alert">
          {problem}
          {view === undefined ? (
            <button
              className="button is-small"
              disabled={busy}
              onClick={() => void act(retry)}
              type="button"
            >
              重试
            </button>
          ) : null}
        </p>
      ) : null}
      <div className="settings-card-actions">
        <button
          className="button is-small"
          disabled={busy || view === undefined}
          onClick={() => void choose()}
          type="button"
        >
          更改…
        </button>
        {view?.chosen ? (
          <button
            className="button is-small"
            disabled={busy}
            onClick={() => void act(() => save(null))}
            type="button"
          >
            恢复默认
          </button>
        ) : null}
      </div>
    </section>
  );
}
