import type { WorkspaceIndexingStatus } from '@aura/contracts/generated';
import { useState } from 'react';
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
      return (
        <span
          className="workspace-index-badge"
          title={`${indexing.unindexed_directories} 个文件夹未建索引`}
        >
          索引不完整
        </span>
      );
    case 'failed':
      return (
        <span className="workspace-index-badge is-failed" title={indexing.failure ?? undefined}>
          索引失败
        </span>
      );
  }
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
  return null;
}

/** Why the build failed, with the way to walk the folder again. */
function FailedIndex({
  workspaceId,
  failure,
}: {
  workspaceId: string;
  failure: string | undefined;
}) {
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<string>();
  const rebuild = async () => {
    setAsking(true);
    setError(undefined);
    try {
      const result = await window.aura.v1.rebuildWorkspaceIndex({ workspace_id: workspaceId });
      if (!result.ok) setError(userMessage(result.error.message, '重建没有开始，请稍后再试。'));
    } catch (reason) {
      setError(userMessage(reason, '重建没有开始，请稍后再试。'));
    } finally {
      setAsking(false);
    }
  };
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
