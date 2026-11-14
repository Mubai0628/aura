import { useRef, useState } from 'react';
import type { AgentAutoContinuedView, AgentTurnView } from '@aura/contracts/generated';
import { userMessage } from './user-message';
import { failedAction } from './failure-text';
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

/** Core's refusals of 继续 that say what to do instead; they carry no sentence of their own. */
const REFUSALS: Record<string, string> = {
  'agent.continuation_requires_checkpoint': '这一轮没有可以接着做的位置，请在输入框说明下一步。',
  'agent.continuation_invalid':
    '这一轮的进度记录对不上，没法从中断处接着做；请在输入框说明下一步。',
};

/** Why 继续 did not go ahead, and what to do. */
function continueFailure(error: unknown): string {
  const text =
    typeof error === 'object' && error !== null
      ? `${String((error as { code?: unknown }).code ?? '')} ${String((error as { message?: unknown }).message ?? '')}`
      : String(error);
  const known = Object.entries(REFUSALS).find(([code]) => text.includes(code));
  return known
    ? known[1]
    : failedAction(error, '没能继续', '任务仍停在这里；可以再点“继续”，或在输入框说明下一步。');
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
      if (!result.ok) setError(continueFailure(result.error));
    } catch (reason) {
      setError(
        userMessage(
          reason,
          '“继续”的结果没有确认（与 Aura Core 的连接出了问题），没有自动重发；请稍后看这一轮是否已经接着在做。',
        ),
      );
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
