import { useEffect, useState } from 'react';

import { MODEL_PAGE_ID, type BrowserAssistState } from '../../../shared/browser-api';
import { AppIcon } from '../app-icons';
import { remainingText } from './assist-time';
import { browserApi, useBrowserPage } from './browser-store';
import { revealModelPage } from './browser-tabs';
import './browser-assist.css';

/**
 * The model waits for the user to act on its browser page. The request
 * sits on top of the composer, which stays usable: the user can still send or steer. Done and
 * Cancel are the user's answer; Aura does not check what they did on the page.
 */
export function BrowserAssist({ sessionId }: { sessionId: string }) {
  const state = useBrowserPage(sessionId, MODEL_PAGE_ID);
  const assist = state?.closed ? undefined : state?.assist;
  // A new request starts its own clock.
  return assist === undefined ? null : (
    <AssistRequest assist={assist} key={assist.deadline} sessionId={sessionId} />
  );
}

function AssistRequest({ assist, sessionId }: { assist: BrowserAssistState; sessionId: string }) {
  const [now, setNow] = useState(Date.now);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  const answer = (kind: 'assist-done' | 'assist-decline') => {
    setBusy(true);
    void browserApi()
      ?.command({ kind, sessionId, pageId: MODEL_PAGE_ID })
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
      <footer className="browser-assist-actions">
        <button
          className="button is-ghost"
          onClick={() => revealModelPage(sessionId)}
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
