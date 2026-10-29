import { useRef, useState } from 'react';
import type { AgentAutoContinuedView, AgentTurnView } from '@aura/contracts/generated';
import { userMessage } from './user-message';
import { restartTime } from './turn-continuation';

/** Why Core restarted, as the note on the turn Aura continued by itself says it. */
const CAUSE: Record<AgentAutoContinuedView['cause'], string> = {
  crash: '意外退出',
  unresponsive: '长时间无响应',
  transport_lost: '连接中断',
  user_restart: '你点了立即重启',
};

/** Above the "继续" Aura sent by itself: when Core restarted, and why. */
export function AutoContinuedNote({ view }: { view: AgentAutoContinuedView }) {
  return (
    <p className="agent-auto-continued" role="note">
      {`Aura Core 在 ${restartTime(view.restarted_at)} 重启（${CAUSE[view.cause]}），已自动继续`}
    </p>
  );
}

/** 继续 for a turn a restart cut off: "继续" goes ahead of the queue, which stays paused. */
export function ContinueButton({
  item,
  onChanged,
}: {
  item: AgentTurnView;
  onChanged: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const sending = useRef(false);
  const press = async () => {
    if (sending.current) return;
    sending.current = true;
    setBusy(true);
    setError(undefined);
    try {
      const result = await window.aura.v1.continueAgent({
        session_id: item.turn.session_id,
        turn_task_id: item.turn.turn_task_id,
      });
      if (!result.ok) setError(userMessage(result.error, '没能继续，请刷新后再试。'));
    } catch (reason) {
      setError(userMessage(reason, '继续的结果未确认，请刷新后核对。'));
    } finally {
      sending.current = false;
      setBusy(false);
      onChanged();
    }
  };
  return (
    <span className="agent-continue">
      <button type="button" disabled={busy} onClick={() => void press()}>
        继续
      </button>
      {error ? <small role="alert">{error}</small> : null}
    </span>
  );
}
