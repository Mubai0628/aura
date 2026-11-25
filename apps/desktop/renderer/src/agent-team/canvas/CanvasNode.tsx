import type { KeyboardEvent, MouseEvent, PointerEvent } from 'react';
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
  onOpen,
  onMenu,
  context,
  ariaLabel,
  selectedForMove,
  graphNode,
  linkSource,
  onKeyDown,
}: {
  facts: NodeFacts;
  at: Point;
  lead?: boolean;
  chosen: boolean;
  context?: string | undefined;
  ariaLabel?: string | undefined;
  selectedForMove?: boolean;
  graphNode?: string;
  linkSource?: boolean;
  onKeyDown?: (event: KeyboardEvent<HTMLButtonElement>) => void;
  onPointerDown: (event: PointerEvent<HTMLButtonElement>) => void;
  onPick: () => void;
  onOpen?: (() => void) | undefined;
  onMenu?: ((event: MouseEvent<HTMLButtonElement>) => void) | undefined;
}) {
  const size = lead ? LEAD : MEMBER;
  return (
    <button
      aria-current={chosen ? 'true' : 'false'}
      aria-pressed={selectedForMove}
      aria-keyshortcuts={selectedForMove === undefined ? undefined : 'Control+Space Enter'}
      data-graph-node={graphNode}
      data-link-source={linkSource || undefined}
      aria-haspopup={onMenu ? 'menu' : undefined}
      aria-label={
        ariaLabel ??
        (context
          ? `${context}的 ${facts.handle ?? ''} ${facts.name}，${facts.model}，${facts.state}，${amount(facts.tokens)} Token${facts.tool ? `，${toolWords(facts.tool)}` : ''}`
          : undefined)
      }
      title={context ? `${context}的 ${facts.handle ?? ''} ${facts.name}` : undefined}
      className={`agent-canvas-node${lead ? ' is-lead' : ''}${facts.ended ? ' is-ended' : ''}`}
      onClick={onPick}
      onDoubleClick={onOpen}
      onContextMenu={onMenu}
      onKeyDown={(event) => {
        if (onOpen && event.key === 'Enter') {
          event.preventDefault();
          onOpen();
        } else onKeyDown?.(event);
      }}
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
        <span className="agent-canvas-tokens" title="会话累计 token">
          {amount(facts.tokens)}
        </span>
      </span>
      <span className="agent-canvas-line">{facts.tool ? toolWords(facts.tool) : ''}</span>
    </button>
  );
}
