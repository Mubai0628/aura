import { useEffect, useId, useRef, useState, type FormEvent } from 'react';

import { MODEL_PAGE_ID, type BrowserUserCommand } from '../../../shared/browser-api';
import { AppIcon } from '../app-icons';
import { browserApi } from './bridge';
import { BrowserHome } from './BrowserHome';
import { browserStore, useBrowserPage } from './browser-store';
import { useNativeView } from './useNativeView';

type Plain = Exclude<BrowserUserCommand['kind'], 'navigate' | 'show' | 'open-file'>;

/** The start page: what a page shows before it loads anything, and after Home. */
const BLANK = 'about:blank';

/**
 * One page of Aura's built-in browser: the toolbar around it and the area Main lays the page
 * over. Home returns to the start page, which a blank page shows instead of
 * an empty white view; who is acting is said above the page, never on it.
 */
export function BrowserPage({
  sessionId,
  pageId,
  visible,
  focus = false,
}: {
  sessionId: string;
  pageId: string;
  visible: boolean;
  /** The user just opened this page: its start page takes the keyboard. */
  focus?: boolean;
}) {
  const state = useBrowserPage(sessionId, pageId);
  const area = useRef<HTMLDivElement>(null);
  const addressId = useId();
  const [draft, setDraft] = useState<string>();
  const [unavailable, setUnavailable] = useState('');
  const model = pageId === MODEL_PAGE_ID;

  const run = (kind: Plain) => {
    void browserApi()?.command({ kind, sessionId, pageId } as BrowserUserCommand);
  };
  const open = () => {
    const api = browserApi();
    if (api === undefined) return;
    void api.command({ kind: 'open', sessionId, pageId }).then((reply) => {
      if (reply.ok) {
        setUnavailable('');
        if (reply.state !== undefined) browserStore.put(reply.state);
      } else setUnavailable(reply.message);
    });
  };
  const go = async (input: string): Promise<string | undefined> => {
    const reply = await browserApi()?.command({ kind: 'navigate', sessionId, pageId, input });
    return reply === undefined || reply.ok ? undefined : reply.message;
  };

  useEffect(() => {
    if (visible) open();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- opened once per page shown
  }, [sessionId, pageId, visible]);

  const home = state !== undefined && !state.closed && state.url === BLANK;
  // The native view is on the page area while it is shown and has something to show.
  useNativeView(
    area,
    sessionId,
    pageId,
    visible && state !== undefined && !state.closed && !home && unavailable === '',
  );

  const navigate = (event: FormEvent) => {
    event.preventDefault();
    const input = draft ?? '';
    setDraft(undefined);
    void go(input).then((refused) => {
      if (refused !== undefined) setDraft(input);
    });
  };

  const url = home ? '' : (state?.url ?? '');
  const loading = state?.loading ?? false;
  const status =
    state === undefined || state.closed
      ? undefined
      : state.assist !== undefined
        ? { tone: 'user', text: '等待你的操作' }
        : state.agentActive
          ? { tone: 'model', text: 'Aura 正在操作' }
          : state.userInControl
            ? { tone: 'user', text: '你在操作' }
            : undefined;
  return (
    <div className="browser-page" data-agent={state?.agentActive ? 'active' : undefined}>
      <div className="browser-toolbar">
        <button
          aria-label="后退"
          className="icon-button"
          disabled={!state?.canGoBack}
          onClick={() => run('back')}
          type="button"
        >
          <AppIcon name="arrow-left" />
        </button>
        <button
          aria-label="前进"
          className="icon-button"
          disabled={!state?.canGoForward}
          onClick={() => run('forward')}
          type="button"
        >
          <AppIcon name="arrow-right" />
        </button>
        <button
          aria-label={loading ? '停止' : '刷新'}
          className="icon-button"
          disabled={state === undefined || state.closed}
          onClick={() => run(loading ? 'stop' : 'reload')}
          type="button"
        >
          <AppIcon name={loading ? 'close' : 'refresh'} />
        </button>
        <button
          aria-label="主页"
          className="icon-button"
          disabled={state === undefined || state.closed || home}
          onClick={() => void go(BLANK)}
          title="主页"
          type="button"
        >
          <AppIcon name="home" />
        </button>
        <form className="browser-address" onSubmit={navigate}>
          <label className="sr-only" htmlFor={addressId}>
            地址
          </label>
          <input
            autoComplete="off"
            id={addressId}
            onBlur={() => setDraft(undefined)}
            onChange={(event) => setDraft(event.target.value)}
            onFocus={(event) => {
              setDraft(url);
              event.currentTarget.select();
            }}
            onKeyDown={(event) => {
              if (event.key === 'Escape') {
                setDraft(undefined);
                event.currentTarget.blur();
              }
            }}
            spellCheck={false}
            value={draft ?? url}
          />
        </form>
        <button
          aria-label="开发者工具"
          className="icon-button"
          disabled={state === undefined || state.closed || home}
          onClick={() => run('devtools')}
          title="开发者工具"
          type="button"
        >
          <AppIcon name="code" />
        </button>
        {model &&
        state !== undefined &&
        !state.closed &&
        (state.assist === undefined || state.userInControl) ? (
          <button
            aria-pressed={state.userInControl}
            className="browser-control-button"
            onClick={() => run(state.userInControl ? 'release-control' : 'take-control')}
            type="button"
          >
            <AppIcon name="hand" size={14} />
            {state.userInControl ? '交还 Aura' : '接管'}
          </button>
        ) : null}
      </div>
      {loading ? (
        <div aria-label="正在加载" className="browser-progress" role="progressbar" />
      ) : null}
      {state?.notice !== undefined && !state.closed ? (
        <div className={`browser-notice is-${state.notice.kind}`} role="alert">
          <AppIcon name="warning" size={14} />
          <span>{state.notice.text}</span>
          <button
            aria-label="关闭提示"
            className="icon-button"
            onClick={() => run('dismiss-notice')}
            type="button"
          >
            <AppIcon name="close" size={13} />
          </button>
        </div>
      ) : null}
      {status === undefined ? null : (
        <div className="browser-status" data-tone={status.tone} role="status">
          {status.tone === 'model' ? (
            <span aria-hidden="true" className="browser-agent-dot" />
          ) : (
            <AppIcon name="hand" size={13} />
          )}
          {status.text}
        </div>
      )}
      <div className="browser-stage">
        <div className="browser-viewport" data-home={home || undefined} ref={area}>
          {unavailable !== '' ? (
            <p className="browser-empty" role="status">
              {unavailable}
            </p>
          ) : state?.closed ? (
            <div className="browser-empty">
              <p role="status">{state.notice?.text ?? '页面已关闭'}</p>
              <button className="secondary-button" onClick={open} type="button">
                重新打开
              </button>
            </div>
          ) : home ? (
            <BrowserHome focus={focus} onOpen={go} />
          ) : null}
        </div>
      </div>
    </div>
  );
}
