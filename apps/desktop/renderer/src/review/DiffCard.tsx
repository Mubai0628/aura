import {
  Fragment,
  memo,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from 'react';
import { AppIcon } from '../app-icons';
import { changedWords, splitRows, type DiffRow, type Segment } from './diff-parse';
import type { PatchReads, PatchState } from './patch-reads';
import { matchId, type ReviewFile } from './review-model';

function Highlighted({
  text,
  query,
  id,
  current,
}: {
  text: string;
  query: string;
  id: string;
  current: boolean;
}) {
  const parts: ReactNode[] = [];
  const lower = text.toLowerCase();
  let from = 0;
  for (let at = lower.indexOf(query); at >= 0; at = lower.indexOf(query, at + query.length)) {
    parts.push(text.slice(from, at));
    parts.push(
      <mark
        className={current ? 'review-match is-current' : 'review-match'}
        // The first mark of a match is where the search scrolls to.
        data-match={at === lower.indexOf(query) ? id : undefined}
        key={at}
      >
        {text.slice(at, at + query.length)}
      </mark>,
    );
    from = at + query.length;
  }
  parts.push(text.slice(from));
  return <>{parts}</>;
}

function RowText({
  row,
  id,
  query,
  current,
  words,
}: {
  row: DiffRow;
  id: string;
  query: string;
  current?: string | undefined;
  words?: Segment[] | undefined;
}) {
  if (query && row.kind !== 'note' && row.text.toLowerCase().includes(query))
    return <Highlighted text={row.text} query={query} id={id} current={current === id} />;
  if (words)
    return (
      <>
        {words.map((word, index) =>
          word.changed ? (
            <span className="diff-word" key={index}>
              {word.text}
            </span>
          ) : (
            <Fragment key={index}>{word.text}</Fragment>
          ),
        )}
      </>
    );
  return <>{row.text}</>;
}

const SIGNS = { add: '+', delete: '−', context: ' ', note: '' } as const;

/** A file's added and removed line counts, or what it is when it has none. */
export function FileCounts({ file }: { file: ReviewFile }) {
  if (file.binary) return <span className="review-counts">二进制</span>;
  if (file.untracked && file.added == null) return <span className="review-counts">新文件</span>;
  return (
    <span className="review-counts" aria-label={`+${file.added ?? 0} −${file.deleted ?? 0}`}>
      <span className="is-added">+{file.added ?? 0}</span>{' '}
      <span className="is-deleted">−{file.deleted ?? 0}</span>
    </span>
  );
}

/**
 * One file's changes on the review page — read when it comes near the screen,
 * shown as lines with their numbers (side by side when asked), the changed words marked.
 */
export const DiffCard = memo(function DiffCard({
  file,
  id,
  reads,
  state,
  collapsed,
  onToggle,
  split,
  wrap,
  words,
  query,
  current,
  scrollRoot,
}: {
  file: ReviewFile;
  id: string;
  reads: PatchReads;
  state: PatchState | undefined;
  collapsed: boolean;
  onToggle: (path: string) => void;
  split: boolean;
  wrap: boolean;
  words: boolean;
  query: string;
  current?: string | undefined;
  /** The list the cards scroll in: nearness is measured against it, not the window. */
  scrollRoot?: RefObject<HTMLElement | null> | undefined;
}) {
  const ref = useRef<HTMLElement>(null);
  // Without an observer (tests), every card is near.
  const [near, setNear] = useState(typeof IntersectionObserver === 'undefined');
  useEffect(() => {
    if (near || collapsed || !ref.current) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        setNear(true);
        observer.disconnect();
      },
      { root: scrollRoot?.current ?? null, rootMargin: '600px' },
    );
    observer.observe(ref.current);
    return () => observer.disconnect();
  }, [near, collapsed, scrollRoot]);
  useEffect(() => {
    if (near && !collapsed) reads.request(file);
  }, [near, collapsed, reads, file]);

  const diff = state?.diff;
  const marked = useMemo(
    () =>
      words && diff
        ? new Map(diff.hunks.flatMap((hunk) => [...changedWords(hunk.rows)]))
        : undefined,
    [words, diff],
  );
  const tag =
    file.untracked || diff?.added
      ? '新文件'
      : diff?.deleted
        ? '已删除'
        : file.original
          ? `改名自 ${file.original}`
          : undefined;

  let body: ReactNode = null;
  if (!collapsed) {
    if (file.binary || diff?.binary)
      body = <p className="review-card-note">二进制文件，不显示差异。</p>;
    else if (!diff && state?.error)
      body = (
        <p className="review-card-note is-error" role="alert">
          {state.error}{' '}
          <button onClick={() => reads.request(file, true)} type="button">
            重试读取
          </button>
        </p>
      );
    else if (!diff)
      body = (
        <p className="review-card-note" role="status">
          正在读取差异…
        </p>
      );
    else if (!diff.hunks.length) body = <p className="review-card-note">没有文本差异。</p>;
    else
      body = (
        <>
          <table className="diff-table" data-mode={split ? 'split' : 'unified'} data-wrap={wrap}>
            <tbody>
              {diff.hunks.map((hunk, h) => {
                const index = new Map(hunk.rows.map((row, r) => [row, r]));
                const text = (row: DiffRow) => {
                  const at = matchId(file.path, `${h}:${index.get(row)}`);
                  return (
                    <RowText
                      row={row}
                      id={at}
                      query={query}
                      current={current}
                      words={marked?.get(row)}
                    />
                  );
                };
                return (
                  <Fragment key={h}>
                    <tr className="diff-hunk">
                      <td colSpan={split ? 4 : 3}>{hunk.header}</td>
                    </tr>
                    {split
                      ? splitRows(hunk.rows).map((pair, p) => (
                          <tr className="diff-row" key={p}>
                            <td className="diff-number">{pair.left?.old ?? ''}</td>
                            <td className={`diff-text is-${pair.left?.kind ?? 'empty'}`}>
                              {pair.left ? text(pair.left) : null}
                            </td>
                            <td className="diff-number">{pair.right?.new ?? ''}</td>
                            <td className={`diff-text is-${pair.right?.kind ?? 'empty'}`}>
                              {pair.right ? text(pair.right) : null}
                            </td>
                          </tr>
                        ))
                      : hunk.rows.map((row, r) => (
                          <tr className={`diff-row is-${row.kind}`} key={r}>
                            <td className="diff-number">{row.old ?? ''}</td>
                            <td className="diff-number">{row.new ?? ''}</td>
                            <td className="diff-text">
                              <span aria-hidden="true" className="diff-sign">
                                {SIGNS[row.kind]}
                              </span>
                              {text(row)}
                            </td>
                          </tr>
                        ))}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
          {state?.error ? (
            <p className="review-card-note is-error" role="alert">
              {state.error}
            </p>
          ) : null}
          {state?.next !== undefined ? (
            <p className="review-card-note">
              差异较长，只显示了一部分。{' '}
              <button disabled={state.loading} onClick={() => reads.more(file)} type="button">
                {state.loading ? '正在读取…' : '继续读取'}
              </button>
            </p>
          ) : null}
        </>
      );
  }

  return (
    <article
      aria-label={`${file.path} 的改动`}
      className="review-card"
      data-collapsed={collapsed}
      id={id}
      ref={ref}
    >
      <header className="review-card-header">
        <button
          aria-expanded={!collapsed}
          className="review-card-toggle"
          onClick={() => onToggle(file.path)}
          title={file.path}
          type="button"
        >
          <AppIcon name={collapsed ? 'chevron-right' : 'chevron-down'} size={13} />
          <span className="review-card-path">
            {query && file.path.toLowerCase().includes(query) ? (
              <Highlighted
                text={file.path}
                query={query}
                id={matchId(file.path, 'path')}
                current={current === matchId(file.path, 'path')}
              />
            ) : (
              file.path
            )}
          </span>
        </button>
        {tag ? <span className="review-tag">{tag}</span> : null}
        {/* A new file's tag already says what its counts would. */}
        {file.untracked && file.added == null ? null : <FileCounts file={file} />}
      </header>
      {file.note ? <p className="review-card-note">{file.note}</p> : null}
      {body}
    </article>
  );
});
