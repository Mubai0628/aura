import type { PathSearchMatch } from '@aura/contracts/generated';
import { useEffect, useState } from 'react';
import { AppIcon } from '../app-icons';
import { userMessage } from '../agent-session/user-message';
import { fileTrees, ROOT, useFileTree, type FileScope } from './file-tree-store';
import { TreeRows, type FileRowMenu } from './TreeRows';
import { FileTreeIndexing } from './workspace-indexing';

export type { FileRowMenu } from './TreeRows';

const lastName = (path: string) => path.slice(path.lastIndexOf('/') + 1);

function FilterResults({
  scope,
  query,
  onPick,
  onReveal,
}: {
  scope: FileScope;
  query: string;
  onPick: (path: string) => void;
  onReveal: (path: string) => void;
}) {
  const [found, setFound] = useState<{
    query: string;
    matches?: PathSearchMatch[];
    error?: string;
  }>();
  useEffect(() => {
    let live = true;
    // Searched once typing pauses, not on every key.
    const timer = setTimeout(() => {
      void window.aura.v1
        .findWorkspacePaths({
          session_id: scope.session,
          workspace_id: scope.workspace,
          query,
          limit: 100,
        })
        .then(
          (result) => {
            if (!live) return;
            setFound(
              result.ok
                ? { query, matches: result.value.matches }
                : { query, error: userMessage(result.error, '没有找到，请重试。') },
            );
          },
          (reason: unknown) => {
            if (live) setFound({ query, error: userMessage(reason, '没有找到，请重试。') });
          },
        );
    }, 200);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [scope.session, scope.workspace, query]);
  if (found?.query !== query) return <li className="file-tree-note">正在查找…</li>;
  if (found.error)
    return (
      <li className="file-tree-note" role="alert">
        {found.error}
      </li>
    );
  if (!found.matches?.length) return <li className="file-tree-note">没有名称相符的文件。</li>;
  return found.matches.map((match) => (
    <li key={match.path}>
      <button
        className={`file-tree-row is-match${match.kind === 'directory' ? '' : ' is-file'}`}
        data-ignored={match.ignored || match.generated ? 'true' : undefined}
        onClick={() => (match.kind === 'directory' ? onReveal(match.path) : onPick(match.path))}
        title={match.path}
        type="button"
      >
        <AppIcon name={match.kind === 'directory' ? 'folder' : 'file'} size={13} />
        <span>{match.path}</span>
      </button>
    </li>
  ));
}

/** The workspace's files, read only — folders open as asked, and a filter. */
export function FileTree({
  id,
  scope,
  current,
  onPick,
  onMenu,
}: {
  /** What the line beside it resizes. */
  id?: string | undefined;
  scope: FileScope;
  /** The file the page shows: its row is marked. */
  current?: string | undefined;
  onPick: (path: string) => void;
  onMenu?: FileRowMenu | undefined;
}) {
  const [filter, setFilter] = useState('');
  const query = filter.trim();
  const tree = useFileTree(scope);
  // The folders a refresh lists again; while one is being read the button shows it.
  const refreshing = [ROOT, ...tree.expanded].some((path) => tree.folders.get(path)?.loading);
  useEffect(() => fileTrees.ensure(scope, ROOT), [scope]);
  return (
    <nav aria-label="文件树" className="file-tree" id={id}>
      <div className="file-tree-filter field-frame">
        <AppIcon name="search" size={13} />
        <input
          aria-label="筛选文件"
          onChange={(event) => setFilter(event.target.value)}
          placeholder="筛选文件"
          type="search"
          value={filter}
        />
        <button
          aria-busy={refreshing}
          aria-label={refreshing ? '正在刷新文件列表' : '刷新文件列表'}
          className={refreshing ? 'icon-button is-refreshing' : 'icon-button'}
          disabled={refreshing}
          onClick={() => fileTrees.refresh(scope)}
          title={refreshing ? '正在刷新文件列表' : '刷新文件列表'}
          type="button"
        >
          <AppIcon name="refresh" size={13} />
        </button>
      </div>
      <FileTreeIndexing workspaceId={scope.workspace} />
      {tree.locating?.error ? (
        <div className="file-tree-note file-tree-locate" role="alert">
          <span title={tree.locating.error}>{tree.locating.error}</span>
          <button onClick={() => fileTrees.retryLocate(scope)} type="button">
            重试
          </button>
          {tree.locating.from ? (
            <button
              onClick={() => {
                setFilter(tree.locating?.from ?? '');
                fileTrees.dismissLocate(scope);
              }}
              type="button"
            >
              重新选择
            </button>
          ) : null}
          <button onClick={() => fileTrees.dismissLocate(scope)} type="button">
            关闭
          </button>
        </div>
      ) : tree.locating ? (
        <p className="file-tree-note file-tree-locate is-pending" role="status">
          正在定位 {lastName(tree.locating.path)}…
        </p>
      ) : null}
      {query ? (
        <ul className="file-tree-list">
          <FilterResults
            scope={scope}
            query={query}
            onPick={onPick}
            onReveal={(path) => {
              fileTrees.locateFolder(scope, path, filter);
              setFilter('');
            }}
          />
        </ul>
      ) : (
        <TreeRows current={current} onMenu={onMenu} onPick={onPick} scope={scope} tree={tree} />
      )}
    </nav>
  );
}
