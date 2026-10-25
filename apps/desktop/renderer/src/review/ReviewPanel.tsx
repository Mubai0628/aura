import {
  Fragment,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react';
import { userMessage } from '../agent-session/user-message';
import { AppIcon } from '../app-icons';
import { DiffCard } from './DiffCard';
import { ReviewFileList } from './ReviewFileList';
import { ReviewToolbar } from './ReviewToolbar';
import {
  arrange,
  KIND_LABELS,
  matchId,
  reviewTotals,
  type Comparison,
  type FileKind,
  type ReviewFile,
} from './review-model';
import { useReviewOptions } from './review-options';
import { keepReviewView, reviewView } from './review-store';
import { useReviewData } from './useReviewData';
import './review.css';

/** Cards shown at a time; more on request. */
const PAGE = 100;
const OPERATIONS: Record<string, string> = {
  merge: '合并',
  rebase: '变基',
  cherry_pick: '拣选提交',
  revert: '还原提交',
  bisect: '二分查找',
};
const none = () => () => {};

/**
 * The review page after the owner's reference, every control working — the
 * comparison (uncommitted changes, all changes against the default branch, or another branch),
 * the file list, search, the view options of the ⋮ menu, filling the main area and closing.
 */
export function ReviewPanel({
  workspaceId,
  sessionId,
  completionKey,
  expanded = false,
  onToggleExpanded,
  onClose,
}: {
  workspaceId: string;
  sessionId: string;
  completionKey?: string | undefined;
  expanded?: boolean | undefined;
  onToggleExpanded?: (() => void) | undefined;
  onClose?: (() => void) | undefined;
}) {
  const options = useReviewOptions();
  // The page opens as it was left in this workspace.
  const [kept] = useState(() => reviewView(workspaceId));
  const [comparison, setComparison] = useState<Comparison>(
    kept?.comparison ?? { kind: 'uncommitted' },
  );
  const review = useReviewData(workspaceId, sessionId, comparison, options.ignoreWhitespace);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(kept?.collapsed ?? new Set());
  const [limit, setLimit] = useState(kept?.limit ?? PAGE);
  const [search, setSearch] = useState<{ query: string; index: number }>();
  const [installationError, setInstallationError] = useState<string>();
  const base = useId();
  const cardsRef = useRef<HTMLDivElement>(null);
  // Where the cards were scrolled to, kept when the page closes.
  const scrollTop = useRef(kept?.scrollTop ?? 0);
  const restored = useRef(false);
  useEffect(() => {
    keepReviewView(workspaceId, { comparison, collapsed, limit, scrollTop: scrollTop.current });
  }, [workspaceId, comparison, collapsed, limit]);
  useLayoutEffect(() => {
    const cards = cardsRef.current;
    if (restored.current || !cards || !review.files.length) return;
    restored.current = true;
    cards.scrollTop = scrollTop.current;
  });

  // What the model changes shows up: a finished tool call reads the page again, once per burst.
  const previousCompletion = useRef(completionKey);
  const refresh = review.refresh;
  useEffect(() => {
    if (previousCompletion.current === completionKey) return;
    previousCompletion.current = completionKey;
    const timer = setTimeout(refresh, 250);
    return () => clearTimeout(timer);
  }, [completionKey, refresh]);

  const reads = review.reads;
  const version = useSyncExternalStore(reads?.subscribe ?? none, () => reads?.version() ?? 0);
  const groups = useMemo(
    () => arrange(review.files, options.separate),
    [review.files, options.separate],
  );
  const ordered = useMemo(() => groups.flatMap((group) => group.files), [groups]);
  const position = useMemo(
    () => new Map(ordered.map((file, index) => [file.path, index])),
    [ordered],
  );

  const query = search?.query.trim().toLowerCase() ?? '';
  const matches = useMemo(() => {
    if (!query) return [];
    const found: string[] = [];
    for (const file of ordered) {
      if (file.path.toLowerCase().includes(query)) found.push(matchId(file.path, 'path'));
      reads?.get(file.path)?.diff?.hunks.forEach((hunk, h) =>
        hunk.rows.forEach((row, r) => {
          if (row.kind !== 'note' && row.text.toLowerCase().includes(query))
            found.push(matchId(file.path, `${h}:${r}`));
        }),
      );
    }
    return found;
    // `version` changes when a diff arrives.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, ordered, reads, version]);
  const index = matches.length ? Math.min(search?.index ?? 0, matches.length - 1) : -1;
  const current = index >= 0 ? matches[index] : undefined;
  const currentPath = current?.slice(0, current.indexOf('\n'));
  // The current match's file shows, open, whatever the page and the collapsed files are.
  const shownCount = Math.max(limit, currentPath ? (position.get(currentPath) ?? 0) + 1 : 0);
  const isCollapsed = (path: string) => collapsed.has(path) && path !== currentPath;

  useEffect(() => {
    if (!current) return;
    const mark = [...(cardsRef.current?.querySelectorAll('[data-match]') ?? [])].find(
      (element) => element.getAttribute('data-match') === current,
    );
    mark?.scrollIntoView({ block: 'center' });
  }, [current, version]);

  const toggle = useCallback(
    (path: string) =>
      setCollapsed((value) => {
        const next = new Set(value);
        if (next.has(path)) next.delete(path);
        else next.add(path);
        return next;
      }),
    [],
  );
  const pick = (file: ReviewFile) => {
    const at = position.get(file.path) ?? 0;
    setCollapsed((value) => {
      const next = new Set(value);
      next.delete(file.path);
      return next;
    });
    setLimit((value) => Math.max(value, at + 1));
    reads?.request(file, true);
    requestAnimationFrame(() =>
      document.getElementById(`${base}-${at}`)?.scrollIntoView({ block: 'start' }),
    );
  };
  const step = (delta: number) =>
    setSearch(
      (value) =>
        value && {
          ...value,
          index: matches.length ? (index + delta + matches.length) % matches.length : 0,
        },
    );

  const status = review.status;
  const operation = status?.operation
    ? (OPERATIONS[status.operation] ?? status.operation)
    : undefined;
  const totals = reviewTotals(review.files);
  const unavailable = review.runtime && review.runtime.state !== 'available';
  // The cards on this page, in their groups.
  const pages: { kind?: FileKind | undefined; files: ReviewFile[] }[] = [];
  let room = shownCount;
  for (const group of groups) {
    const files = group.files.slice(0, room);
    room -= files.length;
    if (files.length) pages.push({ kind: group.kind, files });
  }

  return (
    <section aria-label="Git 审查" className="git-review" data-expanded={expanded}>
      <ReviewToolbar
        comparison={comparison}
        defaultBranch={review.defaultBranch}
        branches={status?.branches ?? []}
        onCompare={(next) => {
          setComparison(next);
          setCollapsed(new Set());
          setLimit(PAGE);
        }}
        options={options}
        searching={search !== undefined}
        onSearch={() => setSearch((value) => (value ? undefined : { query: '', index: 0 }))}
        onCollapseAll={() => setCollapsed(new Set(ordered.map((file) => file.path)))}
        onExpandAll={() => setCollapsed(new Set())}
        onRefresh={refresh}
        expanded={expanded}
        onExpand={() => onToggleExpanded?.()}
        onClose={() => onClose?.()}
      />
      {search ? (
        <div className="review-search" role="search">
          <div className="review-search-field field-frame">
            <AppIcon name="search" size={13} />
            <input
              aria-label="在改动中搜索"
              // The bar opens to be typed in.
              autoFocus
              onChange={(event) => setSearch({ query: event.target.value, index: 0 })}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  step(event.shiftKey ? -1 : 1);
                } else if (event.key === 'Escape') setSearch(undefined);
              }}
              placeholder="搜索文件名与改动"
              type="search"
              value={search.query}
            />
            <span aria-live="polite" className="review-search-count">
              {query ? (matches.length ? `${index + 1}/${matches.length}` : '没有结果') : ''}
            </span>
          </div>
          <button
            aria-label="上一个结果"
            className="icon-button is-up"
            disabled={!matches.length}
            onClick={() => step(-1)}
            type="button"
          >
            <AppIcon name="chevron-down" size={13} />
          </button>
          <button
            aria-label="下一个结果"
            className="icon-button"
            disabled={!matches.length}
            onClick={() => step(1)}
            type="button"
          >
            <AppIcon name="chevron-down" size={13} />
          </button>
          <button
            aria-label="关闭搜索"
            className="icon-button"
            onClick={() => setSearch(undefined)}
            type="button"
          >
            <AppIcon name="close" size={13} />
          </button>
        </div>
      ) : null}
      {status ? (
        <p className="review-status">
          <strong>
            {status.detached
              ? `分离 HEAD · ${status.head?.slice(0, 12) ?? ''}`
              : (status.branch ?? '尚无分支')}
          </strong>
          {!status.head ? ' · 尚无提交' : ''}
          {status.upstream
            ? ` · 上游 ${status.upstream} · 领先 ${status.ahead} · 落后 ${status.behind}`
            : ' · 未设置上游'}
          {operation ? ` · 进行中：${operation}` : ''}
          {status.conflicts.length ? ` · 冲突 ${status.conflicts.length}` : ''}
        </p>
      ) : null}
      {review.loading ? (
        <p className="review-loading" role="status">
          正在读取 Git…
        </p>
      ) : null}
      {review.error ? (
        <p className="git-review-error" role="alert">
          {status ? '刷新失败，保留上次内容：' : '无法读取仓库：'}
          {review.error} 请核对工作区位置和 Git 配置后重试。
        </p>
      ) : null}
      {unavailable ? (
        <div className="git-review-unavailable">
          <h3>Git 不可用</h3>
          <p>{review.runtime?.message}</p>
          <p>{review.runtime?.next_step}</p>
          <button
            onClick={() => {
              void window.aura.v1.git
                .openInstallation()
                .then((result) => {
                  if (!result.ok) throw Error(result.error.message);
                  setInstallationError(undefined);
                })
                .catch((error) => setInstallationError(userMessage(error, '没有打开安装页面。')));
            }}
            type="button"
          >
            打开 Git 安装页面
          </button>
          {installationError ? (
            <p role="alert">{installationError} 请在浏览器中打开 git-scm.com 安装页面。</p>
          ) : null}
        </div>
      ) : null}
      {status && !unavailable ? (
        <>
          <p className="review-summary">
            {review.files.length
              ? `${totals.files} 个文件 · +${totals.added} −${totals.deleted}${
                  totals.uncounted ? `（另有 ${totals.uncounted} 个新文件未计行数）` : ''
                }`
              : comparison.kind === 'uncommitted'
                ? '当前没有未提交的改动。'
                : `与 ${comparison.branch} 相比没有改动。`}
            {review.partial
              ? ` Git 状态共有 ${status.total_entries} 项，未跟踪文件只列出了前 2000 个。`
              : ''}
            {/* What was read last stays; this says Git is being checked behind it. */}
            {review.checking ? (
              <span className="review-checking" role="status">
                正在检查改动…
              </span>
            ) : null}
          </p>
          <div className="review-body" data-list={options.fileList && review.files.length > 0}>
            {options.fileList && review.files.length ? (
              <ReviewFileList groups={groups} folders={options.byFolder} onPick={pick} />
            ) : null}
            <div
              className="review-cards"
              onScroll={(event) => {
                scrollTop.current = event.currentTarget.scrollTop;
                keepReviewView(workspaceId, {
                  comparison,
                  collapsed,
                  limit,
                  scrollTop: scrollTop.current,
                });
              }}
              ref={cardsRef}
            >
              {reads
                ? pages.map((group) => {
                    const cards = group.files;
                    return (
                      <Fragment key={group.kind ?? 'all'}>
                        {group.kind ? (
                          <h3 className="review-cards-group">{KIND_LABELS[group.kind]}</h3>
                        ) : null}
                        {cards.map((file) => (
                          <DiffCard
                            collapsed={isCollapsed(file.path)}
                            current={current}
                            file={file}
                            id={`${base}-${position.get(file.path)}`}
                            key={file.path}
                            onToggle={toggle}
                            query={query}
                            reads={reads}
                            scrollRoot={cardsRef}
                            split={options.split}
                            state={reads.get(file.path)}
                            wrap={options.wrap}
                            words={options.words}
                          />
                        ))}
                      </Fragment>
                    );
                  })
                : null}
              {ordered.length > shownCount ? (
                <button
                  className="review-more-files"
                  onClick={() => setLimit(shownCount + PAGE)}
                  type="button"
                >
                  {`显示更多文件（还有 ${ordered.length - shownCount} 个）`}
                </button>
              ) : null}
            </div>
          </div>
        </>
      ) : null}
    </section>
  );
}
