import type { WorkspaceIndexingStatus } from '@aura/contracts/generated';
import { useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { userMessage } from '../agent-session/user-message';
import { useWorkspaceFiles } from '../workspace-files-store';

/** The workspace's index build while it runs, or when it did not cover the whole tree. */
function useWorkspaceIndexing(
  workspaceId: string | undefined,
): WorkspaceIndexingStatus | undefined {
  const { projection } = useWorkspaceFiles();
  if (workspaceId === undefined) return undefined;
  const status = projection?.payload.workspaces.find((each) => each.workspace_id === workspaceId);
  return status?.catalog.indexing ?? undefined;
}

/** The workspace row's index state: building, not covering the whole tree, or failed. */
export function WorkspaceIndexBadge({ workspaceId }: { workspaceId: string }) {
  const indexing = useWorkspaceIndexing(workspaceId);
  if (indexing === undefined) return null;
  switch (indexing.state) {
    case 'building':
      return (
        <span className="workspace-index-badge" role="status">
          索引中
        </span>
      );
    case 'incomplete':
      return <IncompleteBadge indexing={indexing} />;
    case 'failed':
      return (
        <span className="workspace-index-badge is-failed" title={indexing.failure ?? undefined}>
          索引失败
        </span>
      );
  }
}

/**
 * An index that did not enter every folder: on hover, why, what it means for the model's
 * lookups, and what happens next. The bubble is drawn over the page, out of the sidebar's clip.
 */
function IncompleteBadge({ indexing }: { indexing: WorkspaceIndexingStatus }) {
  const [open, setOpen] = useState(false);
  const badge = useRef<HTMLSpanElement>(null);
  const bubble = useRef<HTMLDivElement>(null);
  const id = useId();
  useLayoutEffect(() => {
    const anchor = badge.current?.getBoundingClientRect();
    const node = bubble.current;
    if (!open || anchor === undefined || node === null) return;
    const bounds = node.getBoundingClientRect();
    node.style.left = `${Math.max(8, Math.min(anchor.left, window.innerWidth - bounds.width - 8))}px`;
    node.style.top = `${anchor.bottom + 6 + bounds.height > window.innerHeight ? anchor.top - bounds.height - 6 : anchor.bottom + 6}px`;
  }, [open]);
  const count = indexing.unindexed_directories;
  const memory = indexing.incomplete_reason !== 'entry_limit';
  return (
    <>
      <span
        aria-describedby={open ? id : undefined}
        className="workspace-index-badge"
        onMouseEnter={() => setOpen(true)}
        onMouseLeave={() => setOpen(false)}
        ref={badge}
        // An empty title keeps the row's own tooltip (its folder) from showing over the bubble.
        title=""
      >
        索引不完整
      </span>
      {open
        ? createPortal(
            <div className="workspace-index-bubble" id={id} ref={bubble} role="tooltip">
              <p>
                {memory
                  ? `建索引时可用内存不足，${count} 个文件夹暂未建索引。`
                  : `文件太多，超过索引上限，${count} 个文件夹未建索引。`}
              </p>
              <p>模型查找、搜索文件时直接读取这些文件夹，结果不受影响。</p>
              <p>
                {memory
                  ? '内存空闲后自动补建，也可以在文件树里立即补建。'
                  : '文件树照常列出这些文件夹。'}
              </p>
            </div>,
            document.body,
          )
        : null}
    </>
  );
}

/** The file tree's line while the index is built, and the reason when the build failed. */
export function FileTreeIndexing({ workspaceId }: { workspaceId: string }) {
  const indexing = useWorkspaceIndexing(workspaceId);
  if (indexing?.state === 'building') {
    return (
      <p className="file-tree-indexing" role="status">
        正在建立文件索引（已处理 {indexing.listed_entries} 项）
      </p>
    );
  }
  if (indexing?.state === 'failed') {
    return <FailedIndex workspaceId={workspaceId} failure={indexing.failure ?? undefined} />;
  }
  if (indexing?.state === 'incomplete' && indexing.incomplete_reason === 'memory_low') {
    if (indexing.walking)
      return (
        <p className="file-tree-indexing" role="status">
          正在补建索引（已处理 {indexing.listed_entries} 项）
        </p>
      );
    return <LeftOutFolders count={indexing.unindexed_directories} workspaceId={workspaceId} />;
  }
  return null;
}

/** Walks the folder again; the reply says only whether the walk started. */
function useRebuild(workspaceId: string, refused: string) {
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<string>();
  const rebuild = async () => {
    setAsking(true);
    setError(undefined);
    try {
      const result = await window.aura.v1.rebuildWorkspaceIndex({ workspace_id: workspaceId });
      if (!result.ok) setError(userMessage(result.error.message, refused));
    } catch (reason) {
      setError(userMessage(reason, refused));
    } finally {
      setAsking(false);
    }
  };
  return { asking, error, rebuild };
}

/** Why the build failed, with the way to walk the folder again. */
function FailedIndex({
  workspaceId,
  failure,
}: {
  workspaceId: string;
  failure: string | undefined;
}) {
  const { asking, error, rebuild } = useRebuild(workspaceId, '重建没有开始，请稍后再试。');
  return (
    <div className="file-tree-indexing is-failed" role="alert">
      <p>文件索引没有建立：{failure ?? '原因未知'}</p>
      <button type="button" disabled={asking} onClick={() => void rebuild()}>
        重建索引
      </button>
      {error ? <p>{error}</p> : null}
    </div>
  );
}

/** Folders a walk left out for want of memory, with the way to fill them in now. */
function LeftOutFolders({ workspaceId, count }: { workspaceId: string; count: number }) {
  const { asking, error, rebuild } = useRebuild(workspaceId, '补建没有开始，请稍后再试。');
  return (
    <div className="file-tree-indexing">
      <p>{count} 个文件夹暂未建索引</p>
      <button type="button" disabled={asking} onClick={() => void rebuild()}>
        立即补建
      </button>
      {error ? <p>{error}</p> : null}
    </div>
  );
}
