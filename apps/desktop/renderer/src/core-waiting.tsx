import { useState } from 'react';
import { AppIcon } from './app-icons';
import { useCoreHealth } from './core-health-store';
import { useWaitedSeconds, waitedText } from './core-waiting-time';
import { userMessage } from './agent-session/user-message';

/** A status that has waited since `since`, counting: "<text>，已等待 N 秒". */
export function WaitingLabel({ text, since }: { text: string; since: string }) {
  const seconds = useWaitedSeconds(since);
  return <>{`${text}，已等待 ${waitedText(seconds)}`}</>;
}

/** Above the composer while Main waits for a ready Core's heartbeat: the wait, and 立即重启. */
export function CoreWaitingNotice() {
  const health = useCoreHealth();
  const payload = health.projection?.payload;
  const since = payload?.state === 'ready' ? (payload.waiting?.since ?? undefined) : undefined;
  // Each wait starts with its own button state.
  return since === undefined ? null : <CoreWaitingBar key={since} since={since} />;
}

function CoreWaitingBar({ since }: { since: string }) {
  const [restarting, setRestarting] = useState(false);
  const [failure, setFailure] = useState<string>();
  const restart = async () => {
    setRestarting(true);
    setFailure(undefined);
    const result = await window.aura.v1.restartCore();
    if (!result.ok) {
      setFailure(userMessage(result.error, '没能重启 Aura Core，请稍后再试。'));
      setRestarting(false);
    }
  };
  return (
    <div className="core-waiting-notice" role="status">
      <AppIcon name="activity" />
      <span>
        <strong>
          <WaitingLabel text="Aura Core 正在处理" since={since} />
        </strong>
        {failure ? <small role="alert">{failure}</small> : null}
      </span>
      <button type="button" disabled={restarting} onClick={() => void restart()}>
        立即重启
      </button>
    </div>
  );
}
