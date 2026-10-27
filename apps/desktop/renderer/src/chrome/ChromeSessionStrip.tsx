import { useCallback, useState } from 'react';
import type { ChromeSessionShares, ChromeSharedTab } from '@aura/contracts/generated';

import type { ResultEnvelope } from '../../../shared/aura-api';
import { AppIcon } from '../app-icons';
import { userMessage } from '../agent-session/user-message';
import { usePolling } from '../ui/usePolling';
import { useReadOrder } from '../ui/useReadOrder';
import { chromeControl } from './bridge';
import '../browser/browser-assist.css';
import './chrome-strip.css';

/** How often the conversation reads its shared tabs while it is open. */
export const SHARES_POLL_MS = 2_000;

type Action = 'stop' | 'finish';

/**
 * The conversation's tabs in the user's own Chrome, above the message box: how many, whether one
 * waits for the user to confirm a site, and Stop. A tab the model handed to the user shows what it
 * asked, with 完成.
 */
export function ChromeSessionStrip({ sessionId }: { sessionId: string }) {
  const [shares, setShares] = useState<ChromeSessionShares>();
  const [pending, setPending] = useState<Action>();
  const [failure, setFailure] = useState<string>();
  const reads = useReadOrder();
  const read = useCallback(
    async (live: () => boolean = () => true) => {
      const asked = reads.begin();
      const result = await chromeControl()?.shares({ session_id: sessionId });
      if (live() && reads.accept(asked) && result?.ok) setShares(result.value);
    },
    [sessionId, reads],
  );
  usePolling(chromeControl() === undefined ? undefined : read, SHARES_POLL_MS);
  if (shares === undefined || shares.tabs.length === 0) return null;
  // The tabs read afterwards show whether it took effect; a failure says why and keeps the button.
  const act = async (
    action: Action,
    work: () => Promise<ResultEnvelope<unknown>> | undefined,
    fallback: string,
  ) => {
    setPending(action);
    try {
      const result = await work();
      if (result === undefined) setFailure(fallback);
      else setFailure(result.ok ? undefined : userMessage(result.error, fallback));
    } catch {
      setFailure(fallback);
    } finally {
      setPending(undefined);
      // A read sent before the action finished may answer with what the action changed.
      reads.settle();
      await read().catch(() => undefined);
    }
  };
  const withUser = shares.tabs.find((tab) => tab.state === 'with_user');
  const awaiting = shares.tabs.some((tab) => tab.state === 'awaiting_origin');
  return (
    <>
      {withUser ? (
        <HandedOver
          onDone={() =>
            void act(
              'finish',
              () => chromeControl()?.finishAssist({ session_id: sessionId }),
              '没有交还给 Aura，请重试。',
            )
          }
          pending={pending}
          tab={withUser}
        />
      ) : null}
      <div aria-label="Chrome" className="chrome-strip" role="status">
        <AppIcon name="browser" size={13} />
        <span className="chrome-strip-count">Chrome · {shares.tabs.length} 个标签页</span>
        {awaiting ? <span className="chrome-strip-badge">等待确认网站</span> : null}
        {shares.connected ? null : <span className="chrome-strip-badge">已断开</span>}
        <button
          className="button is-small chrome-strip-stop"
          disabled={pending !== undefined}
          onClick={() =>
            void act('stop', () => chromeControl()?.stop(), 'Chrome 没有停止，请重试。')
          }
          type="button"
        >
          {pending === 'stop' ? '正在停止…' : '停止'}
        </button>
      </div>
      {failure === undefined ? null : (
        <p className="chrome-strip-failure" role="alert">
          {failure}
        </p>
      )}
    </>
  );
}

function HandedOver({
  tab,
  pending,
  onDone,
}: {
  tab: ChromeSharedTab;
  pending: Action | undefined;
  onDone: () => void;
}) {
  return (
    <section aria-label="Chrome 等待你的操作" className="browser-assist chrome-assist">
      <header className="browser-assist-header">
        <span aria-hidden="true" className="browser-assist-mark">
          <AppIcon name="browser" size={14} />
        </span>
        <strong>Chrome 等待你的操作</strong>
        <span className="chrome-assist-tab">{tab.title || tab.url}</span>
      </header>
      <p className="browser-assist-reason">{tab.assist_reason}</p>
      <footer className="browser-assist-actions">
        <button
          className="button is-primary"
          disabled={pending !== undefined}
          onClick={onDone}
          type="button"
        >
          {pending === 'finish' ? '正在交还…' : '完成'}
        </button>
      </footer>
    </section>
  );
}
