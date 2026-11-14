import { useEffect, useState } from 'react';

import { MODEL_PAGE_ID, type BrowserAssistState } from '../../../shared/browser-api';
import { AppIcon } from '../app-icons';
import { remainingText } from './assist-time';
import { browserApi, useAssistedPage } from './browser-store';
import { reveal, revealModelPage } from './browser-tabs';
import './browser-assist.css';

/**
 * The model waits for the user to act on its browser page, or on a page the user handed to it.
 * The request sits on top of the composer, which stays usable: the user can still send or steer.
 * Done and Cancel are the user's answer; Aura does not check what they did on the page.
 */
export function BrowserAssist({ sessionId }: { sessionId: string }) {
  const state = useAssistedPage(sessionId);
  const assist = state?.assist;
  // A new request starts its own clock.
  return assist === undefined || state === undefined ? null : (
    <AssistRequest
      assist={assist}
      key={assist.deadline}
      pageId={state.pageId}
      sessionId={sessionId}
    />
  );
}

function AssistRequest({
  assist,
  pageId,
  sessionId,
}: {
  assist: BrowserAssistState;
  pageId: string;
  sessionId: string;
}) {
  const [now, setNow] = useState(Date.now);
  const [busy, setBusy] = useState(false);
  /** Why the last answer was not taken. */
  const [refused, setRefused] = useState<string>();
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  const answer = (kind: 'assist-done' | 'assist-decline') => {
    const api = browserApi();
    if (api === undefined) return;
    setBusy(true);
    setRefused(undefined);
    void api
      .command({ kind, sessionId, pageId })
      .then((result) => {
        if (!result.ok) setRefused(result.message);
      })
      .finally(() => setBusy(false));
  };
  return (
    <section aria-label="浏览器等待你的操作" className="browser-assist">
      <header className="browser-assist-header">
        <span aria-hidden="true" className="browser-assist-mark">
          <AppIcon name="browser" size={14} />
        </span>
        <strong>浏览器等待你的操作</strong>
        <span className="browser-assist-time" role="timer">
          剩余 {remainingText(assist.deadline - now)}
        </span>
      </header>
      <p className="browser-assist-reason">{assist.reason}</p>
      {refused === undefined ? null : (
        <p className="browser-assist-refused" role="alert">
          {refused}
        </p>
      )}
      <footer className="browser-assist-actions">
        <button
          className="button is-ghost"
          onClick={() =>
            pageId === MODEL_PAGE_ID ? revealModelPage(sessionId) : reveal(sessionId, pageId)
          }
          type="button"
        >
          打开浏览器
        </button>
        <button
          className="button is-primary"
          disabled={busy}
          onClick={() => answer('assist-done')}
          type="button"
        >
          完成
        </button>
        <button
          className="button is-ghost"
          disabled={busy}
          onClick={() => answer('assist-decline')}
          type="button"
        >
          取消
        </button>
      </footer>
    </section>
  );
}
