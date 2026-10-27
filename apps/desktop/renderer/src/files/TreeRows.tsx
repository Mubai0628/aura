import type { CSSProperties, MouseEvent } from 'react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { AppIcon } from '../app-icons';
import { fileTrees, type FileScope, type TreeState } from './file-tree-store';
import { entryKey, treeRows, type TreeRow } from './tree-rows';

/** A file row's right-click menu, when the tree offers one. */
export type FileRowMenu = (event: MouseEvent<HTMLElement>, path: string) => void;

/** Every line's height in pixels, as files.css draws it (`--file-row-height`). */
export const ROW_HEIGHT = 24;
/** The list's height assumed until it is measured. */
const UNMEASURED = 640;

const placed = (index: number, depth: number) =>
  ({ top: index * ROW_HEIGHT, '--file-depth': depth }) as CSSProperties;

const later = (work: () => void) => {
  if (typeof requestAnimationFrame === 'function') requestAnimationFrame(work);
  else setTimeout(work);
};

/**
 * The open folders' lines, drawn only near the view: a screen above it, the view and a screen
 * below — however many entries the folders hold — plus the line focused and the lines beside it,
 * the line located and the open file's line, which stay drawn while they are worked with. Lines
 * are placed by index, so the list scrolls as if every line were there.
 */
export function TreeRows({
  scope,
  tree,
  current,
  onPick,
  onMenu,
}: {
  scope: FileScope;
  tree: TreeState;
  /** The file the page shows: its line is marked and kept in view. */
  current?: string | undefined;
  onPick: (path: string) => void;
  onMenu?: FileRowMenu | undefined;
}) {
  const rows = useMemo(() => treeRows(tree), [tree]);
  const places = useMemo(() => new Map(rows.map((row, index) => [row.key, index])), [rows]);
  const box = useRef<HTMLDivElement>(null);
  const [view, setView] = useState({ top: 0, height: UNMEASURED });
  const [focused, setFocused] = useState<string>();
  const measure = () => {
    const element = box.current;
    if (element) setView({ top: element.scrollTop, height: element.clientHeight || UNMEASURED });
  };
  useEffect(() => {
    const element = box.current;
    if (!element || typeof ResizeObserver !== 'function') return;
    const observer = new ResizeObserver(() =>
      setView({ top: element.scrollTop, height: element.clientHeight || UNMEASURED }),
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  // The located line comes to the middle once it is laid out, also when its folders are read
  // after the request; each locate once, so a later page or a reopened folder does not jump to it.
  const centred = useRef<number | undefined>(undefined);
  useEffect(() => {
    const element = box.current;
    const target = tree.located;
    const asked = tree.locatedAt;
    if (!element || !target || asked === undefined || centred.current === asked) return;
    const at = places.get(entryKey(target));
    if (at === undefined) return;
    centred.current = asked;
    element.scrollTop = Math.max(0, at * ROW_HEIGHT - (element.clientHeight - ROW_HEIGHT) / 2);
    later(() => {
      const now = box.current;
      if (now) setView({ top: now.scrollTop, height: now.clientHeight || UNMEASURED });
    });
  }, [tree.located, tree.locatedAt, places]);
  // The open file's line is kept in view when another file opens.
  const shownOpen = useRef<string | undefined>(undefined);
  useEffect(() => {
    const element = box.current;
    if (!element || !current || shownOpen.current === current) return;
    const at = places.get(entryKey(current));
    if (at === undefined) return;
    shownOpen.current = current;
    const top = at * ROW_HEIGHT;
    const height = element.clientHeight || UNMEASURED;
    if (top < element.scrollTop) element.scrollTop = top;
    else if (top + ROW_HEIGHT > element.scrollTop + height)
      element.scrollTop = top + ROW_HEIGHT - height;
    later(() => {
      const now = box.current;
      if (now) setView({ top: now.scrollTop, height: now.clientHeight || UNMEASURED });
    });
  }, [current, places]);

  // A list that got shorter below the view (a folder closed) is scrolled back by the browser,
  // which need not say so: the view is measured again, and its last screen drawn meanwhile.
  const lines = rows.length;
  useEffect(() => {
    if (box.current && box.current.scrollTop !== view.top) measure();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- measured when the line count changes.
  }, [lines]);
  const screen = Math.max(1, Math.ceil(view.height / ROW_HEIGHT));
  const first = Math.min(Math.floor(view.top / ROW_HEIGHT), Math.max(0, lines - screen));
  const from = Math.max(0, first - screen);
  const to = Math.min(rows.length, first + 2 * screen);
  const shown: number[] = [];
  for (let index = from; index < to; index += 1) shown.push(index);
  // The focused line keeps the lines beside it, so Tab and Shift+Tab still reach its neighbours
  // after the list scrolled away from it.
  const at = (key: string | undefined) => (key ? places.get(key) : undefined);
  const near = at(focused);
  const kept = [
    ...(near === undefined ? [] : [near - 1, near, near + 1]),
    at(tree.located && entryKey(tree.located)),
    at(current && entryKey(current)),
  ];
  for (const index of kept) {
    if (index === undefined || index < 0 || index >= rows.length) continue;
    if ((index < from || index >= to) && !shown.includes(index)) shown.push(index);
  }
  shown.sort((left, right) => left - right);

  return (
    <div
      className="file-tree-list"
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null))
          setFocused(undefined);
      }}
      onFocus={(event) =>
        setFocused((event.target as HTMLElement).closest('li')?.dataset.row ?? undefined)
      }
      onScroll={measure}
      ref={box}
    >
      <ul className="file-tree-rows" style={{ height: rows.length * ROW_HEIGHT }}>
        {shown.map((index) => (
          <Line
            current={current}
            index={index}
            key={rows[index]!.key}
            located={tree.located}
            onMenu={onMenu}
            onPick={onPick}
            row={rows[index]!}
            scope={scope}
          />
        ))}
      </ul>
    </div>
  );
}

function Line({
  row,
  index,
  scope,
  located,
  current,
  onPick,
  onMenu,
}: {
  row: TreeRow;
  index: number;
  scope: FileScope;
  located: string | undefined;
  current: string | undefined;
  onPick: (path: string) => void;
  onMenu?: FileRowMenu | undefined;
}) {
  const style = placed(index, row.depth);
  if (row.kind === 'reading')
    return (
      <li className="file-tree-note" data-row={row.key} style={style}>
        正在读取…
      </li>
    );
  if (row.kind === 'empty')
    return (
      <li className="file-tree-note" data-row={row.key} style={style}>
        空文件夹
      </li>
    );
  if (row.kind === 'error')
    return (
      <li className="file-tree-note" data-row={row.key} role="alert" style={style}>
        <span title={row.error}>{row.error}</span>
        <button
          aria-label="重新读取文件列表"
          onClick={() => void fileTrees.load(scope, row.path)}
          type="button"
        >
          重试
        </button>
      </li>
    );
  if (row.kind === 'more')
    return (
      <li className="file-tree-note" data-row={row.key} style={style}>
        <button
          className="file-tree-more"
          disabled={row.folder.loadingMore === true}
          onClick={() => void fileTrees.more(scope, row.path)}
          type="button"
        >
          {row.folder.loadingMore ? '正在加载…' : '加载更多'}
        </button>
        {row.folder.moreError ? (
          <span role="alert" title={row.folder.moreError}>
            {row.folder.moreError}
          </span>
        ) : null}
      </li>
    );
  const entry = row.entry;
  const quiet = entry.ignored || entry.generated ? 'true' : undefined;
  if (entry.kind === 'directory')
    return (
      <li data-row={row.key} style={style}>
        <button
          aria-current={located === entry.path || undefined}
          aria-expanded={row.expanded}
          className="file-tree-row"
          data-ignored={quiet}
          onClick={() => fileTrees.toggle(scope, entry.path)}
          title={entry.path}
          type="button"
        >
          <AppIcon name={row.expanded ? 'chevron-down' : 'chevron-right'} size={13} />
          <span>{entry.name}</span>
        </button>
      </li>
    );
  return (
    <li data-row={row.key} style={style}>
      <button
        aria-current={located === entry.path || undefined}
        className="file-tree-row is-file"
        data-ignored={quiet}
        data-open={current === entry.path || undefined}
        onClick={() => onPick(entry.path)}
        onContextMenu={onMenu ? (event) => onMenu(event, entry.path) : undefined}
        title={entry.path}
        type="button"
      >
        <AppIcon name="file" size={13} />
        <span>{entry.name}</span>
      </button>
    </li>
  );
}
