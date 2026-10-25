import type { ReactNode, RefObject } from 'react';
import { useEffect, useRef, useState } from 'react';
import { AppIcon } from '../app-icons';
import { MarkdownBody } from '../markdown-body';
import { isBareName } from './file-locate';
import { basename, isMarkdownPath, mediaKind } from './file-paths';
import { formatBytes, PAGE, readFile, readLines, type FileContent } from './file-reads';
import { entryName, inWorkspace } from './file-sources';
import type { FileScope } from './file-tree-store';
import { MediaView } from './MediaView';
import { SameNameFiles } from './SameNameFiles';

/** Lines shown at a time; more on request. */
const STEP = 2000;

function Lines({
  lines,
  limit,
  target,
  targetRef,
}: {
  lines: string[];
  limit: number;
  target?: number | undefined;
  targetRef: RefObject<HTMLDivElement | null>;
}) {
  return (
    <div className="file-text">
      {lines.slice(0, limit).map((text, index) => (
        <div
          aria-current={index + 1 === target ? 'true' : undefined}
          className="file-line"
          data-line={index + 1}
          key={index}
          ref={index + 1 === target ? targetRef : undefined}
        >
          <span className="file-line-number">{index + 1}</span>
          <span className="file-line-text">{text}</span>
        </div>
      ))}
    </div>
  );
}

/**
 * One file, read only. Text shows with line numbers; Markdown opens as a
 * preview and one button shows its source; a picture shows as itself. What the model changes is
 * read again when a tool call finishes, while the tab is on screen.
 */
export function FileView({
  scope,
  path,
  line,
  shown,
  visible,
  refreshKey,
  leading,
}: {
  scope: FileScope;
  path: string;
  line?: number | undefined;
  /** Changes each time the line is asked for, so it is shown again. */
  shown?: number | undefined;
  visible: boolean;
  /** Changes when a tool call finishes. */
  refreshKey?: string | undefined;
  /** The tree's button, at the start of the header. */
  leading?: ReactNode;
}) {
  const markdown = isMarkdownPath(path);
  const [attempt, setAttempt] = useState(0);
  // The refresh the content reflects: a hidden tab reads again only once it is shown.
  const [seen, setSeen] = useState(refreshKey);
  // A picture, a video or a sound is not read again on its own: a playing video would start over.
  const wanted = visible && mediaKind(path) === undefined ? refreshKey : seen;
  const key = `${path}\n${attempt}\n${wanted ?? ''}`;
  const [result, setResult] = useState<{ key: string; content: FileContent }>();
  const [more, setMore] = useState<{ busy: boolean; error?: string }>({ busy: false });
  const [limit, setLimit] = useState(STEP);
  const [choice, setChoice] = useState<{
    shown?: number | undefined;
    mode: 'preview' | 'source';
  }>();
  const targetRef = useRef<HTMLDivElement>(null);
  const [actualSize, setActualSize] = useState(false);
  // A new request for a line shows the source; the user's own choice holds until the next one.
  const mode = !markdown
    ? 'source'
    : choice && choice.shown === shown
      ? choice.mode
      : line
        ? 'source'
        : 'preview';

  useEffect(() => {
    let live = true;
    void readFile({ session: scope.session, workspace: scope.workspace }, path).then((content) => {
      if (!live) return;
      setResult({ key, content });
      setSeen(wanted);
    });
    return () => {
      live = false;
    };
  }, [key, scope.session, scope.workspace, path, wanted]);

  const content = result?.content;
  useEffect(() => {
    if (line && visible && content?.kind === 'text' && mode === 'source')
      targetRef.current?.scrollIntoView({ block: 'center' });
  }, [line, shown, visible, content, mode]);

  const readMore = async () => {
    if (content?.kind !== 'text') return;
    setMore({ busy: true });
    const part = await readLines(scope, path, content.lines.length);
    if (!part.ok) {
      setMore({ busy: false, error: part.message });
      return;
    }
    setResult((current) =>
      current?.content.kind === 'text'
        ? {
            key: current.key,
            content: {
              ...current.content,
              lines: [...current.content.lines, ...part.lines],
              total: part.total,
            },
          }
        : current,
    );
    setLimit((value) => Math.max(value, content.lines.length + PAGE));
    setMore({ busy: false });
  };

  let body: ReactNode;
  if (!content)
    body = (
      <p className="file-view-status" role="status">
        正在读取…
      </p>
    );
  else if (content.kind === 'error')
    body = (
      <div className="file-view-status" role="alert">
        <p>{content.message}</p>
        <button onClick={() => setAttempt((value) => value + 1)} type="button">
          重新读取
        </button>
        {isBareName(path) ? <SameNameFiles line={line} name={path} scope={scope} /> : null}
      </div>
    );
  else if (content.kind === 'binary')
    body = (
      <div className="file-view-status">
        <AppIcon name="file" size={28} />
        <p>{content.note ?? '这个文件不是文本，暂不能预览。'}</p>
        <p className="file-view-facts">{`${content.description} · ${formatBytes(content.size)}`}</p>
      </div>
    );
  else if (content.kind === 'media')
    body = (
      <MediaView
        actual={actualSize}
        key={content.src}
        media={content.media}
        name={basename(path)}
        path={path}
        scope={scope}
        src={content.src}
        visible={visible}
      />
    );
  else if (mode === 'preview')
    body = <MarkdownBody className="file-markdown" text={content.lines.join('\n')} />;
  else {
    const showing = Math.min(content.lines.length, Math.max(limit, line ?? 0));
    body = (
      <>
        <Lines lines={content.lines} limit={showing} target={line} targetRef={targetRef} />
        {showing < content.lines.length ? (
          <button className="file-view-more" onClick={() => setLimit(showing + STEP)} type="button">
            {`显示更多（还有 ${content.lines.length - showing} 行）`}
          </button>
        ) : content.paged && content.lines.length < content.total ? (
          <button
            className="file-view-more"
            disabled={more.busy}
            onClick={() => void readMore()}
            type="button"
          >
            {more.busy
              ? '正在读取…'
              : `继续读取（还有 ${content.total - content.lines.length} 行）`}
          </button>
        ) : null}
        {more.error ? (
          <p className="file-view-status" role="alert">
            {more.error}
          </p>
        ) : null}
        {content.note ? <p className="file-view-status">{content.note}</p> : null}
      </>
    );
  }

  return (
    <div className="file-view">
      <header className="file-view-header">
        {leading}
        <span className="file-view-path" title={inWorkspace(path) ? path : entryName(path)}>
          {inWorkspace(path) ? path : entryName(path)}
        </span>
        {markdown && content?.kind === 'text' ? (
          <button
            aria-label={mode === 'preview' ? '查看源码' : '查看预览'}
            className="icon-button"
            onClick={() => setChoice({ shown, mode: mode === 'preview' ? 'source' : 'preview' })}
            title={mode === 'preview' ? '查看源码' : '查看预览'}
            type="button"
          >
            <AppIcon name={mode === 'preview' ? 'code' : 'eye'} size={15} />
          </button>
        ) : null}
        {content?.kind === 'media' && content.media === 'image' ? (
          <button
            aria-label={actualSize ? '适应面板' : '原始尺寸'}
            aria-pressed={actualSize}
            className="icon-button"
            onClick={() => setActualSize((value) => !value)}
            title={actualSize ? '适应面板' : '原始尺寸'}
            type="button"
          >
            <AppIcon name={actualSize ? 'shrink' : 'expand'} size={15} />
          </button>
        ) : null}
        <button
          aria-label="刷新文件"
          className="icon-button"
          onClick={() => setAttempt((value) => value + 1)}
          title="刷新文件"
          type="button"
        >
          <AppIcon name="refresh" size={15} />
        </button>
      </header>
      <div className="file-view-body">{body}</div>
    </div>
  );
}
