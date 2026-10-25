import { useEffect, useState } from 'react';
import type { AgentTurnView, HookInvocationList } from '@aura/contracts/generated';
import { hookEventLabel } from '../hook-settings/hook-labels';
import { terminalTurn } from './turn-state';
import './hook-turn-summary.css';

/** `turn.completed` Hooks run just after the turn ends, so a fresh turn is read once more. */
const SETTLE_MS = 2_000;
const FRESH_MS = 10_000;

/** When a turn has ended: the Hook that stopped something in it, and one line of totals. */
export function HookTurnSummary({ item }: { item: AgentTurnView }) {
  const terminal = terminalTurn(item.turn.state.state);
  const turn = item.turn.turn_task_id;
  const updated = item.turn.updated_at;
  const [list, setList] = useState<HookInvocationList>();
  useEffect(() => {
    if (!terminal) return;
    let live = true;
    const read = () => {
      try {
        void window.aura.v1.listHookInvocations({ path: null, turn_task_id: turn, limit: 20 }).then(
          (result) => {
            if (live && result.ok) setList(result.value);
          },
          () => undefined,
        );
      } catch {
        // A bridge without Hooks shows nothing; the turn itself is unaffected.
      }
    };
    read();
    const fresh = Date.now() - Date.parse(updated) < FRESH_MS;
    const timer = fresh ? setTimeout(read, SETTLE_MS) : undefined;
    return () => {
      live = false;
      if (timer) clearTimeout(timer);
    };
  }, [terminal, turn, updated]);
  const statistics = list?.statistics;
  if (!terminal || !statistics || statistics.calls === 0) return null;
  const blocked = list.invocations.find((call) => call.effect === 'action_blocked');
  return (
    <div className="agent-hook-summary">
      {blocked ? (
        <p role="status">
          被 Hook {blocked.hook_name} 阻止（{hookEventLabel(blocked.event)}）：
          {blocked.reason ?? '没有说明原因'}
        </p>
      ) : null}
      <small>
        Hook：调用 {statistics.calls} 次，成功 {statistics.succeeded}，失败 {statistics.failed}
        ，阻止 {statistics.blocked}
      </small>
    </div>
  );
}
