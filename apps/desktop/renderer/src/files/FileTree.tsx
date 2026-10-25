import type { CSSProperties, MouseEvent } from 'react';
import type { PathSearchMatch } from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';
import { AppIcon } from '../app-icons';
import { userMessage } from '../agent-session/user-message';
import { fileTrees, ROOT, useFileTree, type FileScope } from './file-tree-store';

/** A file row's right-click menu, when the tree offers one. */
export type FileRowMenu = (event: MouseEvent<HTMLElement>, path: string) => void;

/** A folder shows at most this many entries; the filter finds the rest. */
const SHOWN = 1000;

const depthStyle = (depth: number) => ({ '--file-depth': depth }) as CSSProperties;

/** A file's row. The one 在文件树中显示 asked for is marked and brought to the middle; the file the page shows is marked too, and kept in view. */
function FileRow({
  path,
  name,
  depth,
  ignored,
  located,
  open,
  onPick,
  onMenu,
}: {
  path: string;
  name: string;
  depth: number;
  ignored: boolean;
  located: boolean;
  open: boolean;
  onPick: (path: string) => void;
  onMenu?: FileRowMenu | undefined;
}) {
  const ref = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (located) ref.current?.scrollIntoView?.({ block: 'center' });
  }, [located]);
  useEffect(() => {
    if (open) ref.current?.scrollIntoView?.({ block: 'nearest' });
  }, [open]);
  return (
    <li>
      <button
        aria-current={located || undefined}
        className="file-tree-row is-file"
        data-ignored={ignored ? 'true' : undefined}
        data-open={open || undefined}
        onClick={() => onPick(path)}
        onContextMenu={onMenu ? (event) => onMenu(event, path) : undefined}
        ref={ref}
        style={depthStyle(depth)}
        title={path}
        type="button"
      >
        <AppIcon name="file" size={13} />
        <span>{name}</span>
      </button>
    </li>
  );
}

function FolderRows({
  scope,
  path,
  depth,
  current,
  onPick,
  onMenu,
}: {
  scope: FileScope;
  path: string;
  depth: number;
  /** The file the page shows. */
  current?: string | undefined;
  onPick: (path: string) => void;
  onMenu?: FileRowMenu | undefined;
}) {
  const tree = useFileTree(scope);
  const folder = tree.folders.get(path);
  if (folder?.error && !folder.loading)
    return (
      <li className="file-tree-note" role="alert" style={depthStyle(depth)}>
        <span>{folder.error}</span>
        <button
          aria-label="重新读取文件列表"
          onClick={() => void fileTrees.load(scope, path)}
          type="button"
        >
          重试
        </button>
      </li>
    );
  if (!folder?.entries)
    return (
      <li className="file-tree-note" style={depthStyle(depth)}>
        正在读取…
      </li>
    );
  const entries = folder.entries;
  return (
    <>
      {entries.length === 0 ? (
        <li className="file-tree-note" style={depthStyle(depth)}>
          空文件夹
        </li>
      ) : null}
      {entries.slice(0, SHOWN).map((entry) =>
        entry.kind === 'directory' ? (
          <li key={entry.path}>
            <button
              aria-expanded={tree.expanded.has(entry.path)}
              className="file-tree-row"
              data-ignored={entry.ignored || entry.generated ? 'true' : undefined}
              onClick={() => fileTrees.toggle(scope, entry.path)}
              style={depthStyle(depth)}
              title={entry.path}
              type="button"
            >
              <AppIcon
                name={tree.expanded.has(entry.path) ? 'chevron-down' : 'chevron-right'}
                size={13}
              />
              <span>{entry.name}</span>
            </button>
            {tree.expanded.has(entry.path) ? (
              <ul>
                <FolderRows
                  scope={scope}
                  path={entry.path}
                  depth={depth + 1}
                  current={current}
                  onPick={onPick}
                  onMenu={onMenu}
                />
              </ul>
            ) : null}
          </li>
        ) : (
          <FileRow
            depth={depth}
            ignored={entry.ignored || entry.generated}
            key={entry.path}
            located={tree.located === entry.path}
            name={entry.name}
            open={current === entry.path}
            onMenu={onMenu}
            onPick={onPick}
            path={entry.path}
          />
        ),
      )}
      {entries.length > SHOWN ? (
        <li className="file-tree-note" style={depthStyle(depth)}>
          还有 {entries.length - SHOWN} 项未列出，可用上方筛选查找。
        </li>
      ) : null}
    </>
  );
}

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
          aria-label="刷新文件列表"
          className="icon-button"
          onClick={() => fileTrees.refresh(scope)}
          title="刷新文件列表"
          type="button"
        >
          <AppIcon name="refresh" size={13} />
        </button>
      </div>
      <ul className="file-tree-list">
        {query ? (
          <FilterResults
            scope={scope}
            query={query}
            onPick={onPick}
            onReveal={(path) => {
              fileTrees.reveal(scope, path);
              setFilter('');
            }}
          />
        ) : (
          <FolderRows
            scope={scope}
            path={ROOT}
            depth={0}
            current={current}
            onPick={onPick}
            onMenu={onMenu}
          />
        )}
      </ul>
    </nav>
  );
}
