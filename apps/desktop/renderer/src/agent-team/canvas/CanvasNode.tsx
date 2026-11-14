import type { MouseEvent, PointerEvent } from 'react';
import { amount, toolWords } from '../agent-team-words';
import { LEAD, MEMBER, type Point } from './layout';
import type { NodeFacts } from './node-facts';

/** An agent's card: picking it shows that agent in the main area; it can be moved, and a
 *  sub-agent's card has a menu. */
export function CanvasNode({
  facts,
  at,
  lead = false,
  chosen,
  onPointerDown,
  onPick,
  onMenu,
}: {
  facts: NodeFacts;
  at: Point;
  lead?: boolean;
  chosen: boolean;
  onPointerDown: (event: PointerEvent<HTMLButtonElement>) => void;
  onPick: () => void;
  onMenu?: ((event: MouseEvent<HTMLButtonElement>) => void) | undefined;
}) {
  const size = lead ? LEAD : MEMBER;
  return (
    <button
      aria-current={chosen ? 'true' : 'false'}
      className={`agent-canvas-node${lead ? ' is-lead' : ''}${facts.ended ? ' is-ended' : ''}`}
      onClick={onPick}
      onContextMenu={onMenu}
      onPointerDown={onPointerDown}
      style={{ left: at.x, top: at.y, width: size.width, height: size.height }}
      type="button"
    >
      {facts.approvals ? <span className="agent-canvas-badge">{facts.approvals}</span> : null}
      {facts.branch ? <span className="agent-canvas-tab">{facts.branch}</span> : null}
      <span className="agent-canvas-head">
        <strong>{facts.name}</strong>
        {facts.handle ? <small>{facts.handle}</small> : null}
      </span>
      <span className="agent-canvas-line">{facts.model}</span>
      <span className="agent-canvas-foot">
        <span className="agent-team-state" data-phase={facts.phase}>
          {facts.state}
        </span>
        <span className="agent-canvas-tokens">{amount(facts.tokens)}</span>
      </span>
      <span className="agent-canvas-line">{facts.tool ? toolWords(facts.tool) : ''}</span>
    </button>
  );
}
