import { useEffect, useState, type ReactNode } from 'react';
import type { AgentTurnView } from '@aura/contracts/generated';

import { ExpandIcon, ToolIcon } from '../tool-icon';
import { turnDuration } from './turn-duration';
import { terminalTurn } from './turn-state';

export function TurnStatus({
  item,
  label,
  actions,
  toggle,
}: {
  item: AgentTurnView;
  label: string;
  /** Controls at the end of the line, such as opening the turn's inspection. */
  actions?: ReactNode;
  /** A finished turn's line opens and closes its process. */
  toggle?: { open: boolean; onToggle: () => void; controls: string } | undefined;
}) {
  const terminal = terminalTurn(item.turn.state.state);
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (terminal) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [terminal]);
  // A finished turn's time is at the right of the line under its answer (TurnSummary).
  const duration = terminal ? undefined : turnDuration(item, now);
  const running = item.turn.state.state === 'running';
  return (
    <div className="agent-turn-status" data-state={item.turn.state.state}>
      {running ? (
        // A thin arc turning in the slot the
        // finished turn's mark takes, the same kind as the stop button's ring.
        <span className="agent-turn-symbol" aria-hidden="true">
          <span className="agent-turn-spinner" />
        </span>
      ) : (
        <span className="agent-turn-symbol" aria-hidden="true">
          <ToolIcon
            name={
              item.turn.state.state === 'completed' ? 'check' : terminal ? 'circle-alert' : 'pause'
            }
          />
        </span>
      )}
      {toggle ? (
        <button
          type="button"
          className="agent-turn-toggle"
          aria-expanded={toggle.open}
          aria-controls={toggle.controls}
          title={toggle.open ? '收起过程' : '展开过程'}
          onClick={toggle.onToggle}
        >
          <span role="status">{label}</span>
          {duration ? (
            <span className="agent-elapsed" aria-label={`耗时 ${duration}`}>
              {duration}
            </span>
          ) : null}
          <ExpandIcon open={toggle.open} />
        </button>
      ) : (
        <>
          <span role="status">{label}</span>
          {duration ? (
            <span className="agent-elapsed" aria-label={`耗时 ${duration}`}>
              {duration}
            </span>
          ) : null}
        </>
      )}
      {actions}
    </div>
  );
}
