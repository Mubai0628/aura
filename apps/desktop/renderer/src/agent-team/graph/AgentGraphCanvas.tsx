import type { AgentGraphView } from '@aura/contracts/generated';
import { useId, useMemo, type KeyboardEvent } from 'react';
import { AppIcon } from '../../app-icons';
import { useContextMenu, type MenuEntry } from '../../ui/useContextMenu';
import { GraphParentNodes } from './GraphParentNodes';
import { GraphPeerLines } from './GraphPeerLines';
import { graphPositions } from './graph-layout';
import { graphRevision, type GraphChange } from './useAgentGraph';
import { useGraphCamera } from './useGraphCamera';
import { useGraphPlacement } from './useGraphPlacement';
import { useGraphLink } from './useGraphLink';
import { draggedSession, SESSION_DRAG } from './drag-data';
import '../canvas/agent-team-canvas.css';

const DIRECTION: Record<string, readonly [number, number]> = {
  ArrowLeft: [-1, 0],
  ArrowRight: [1, 0],
  ArrowUp: [0, -1],
  ArrowDown: [0, 1],
};

export function AgentGraphCanvas({
  view,
  sessionId,
  chosen,
  disabled,
  change,
  onPick,
  onRemove,
  onAdd,
  onReset,
  onRefresh,
  onEditProfile,
}: {
  view: AgentGraphView;
  sessionId: string;
  chosen: string | undefined;
  disabled: boolean;
  change: GraphChange;
  onPick: (parent: string, agent?: string) => void;
  onRemove: (parent: string) => void;
  onAdd: (parent: string) => void;
  onReset: () => void;
  onRefresh: () => void;
  onEditProfile: (parent: string) => void;
}) {
  const saved = useMemo(() => graphPositions(view), [view]);
  const {
    camera,
    boardRef,
    freeze,
    zoom: zoomBy,
    showAll,
    panStart,
    panMove,
    panEnd,
    panBy,
  } = useGraphCamera(saved);
  const visibleNodes = () =>
    Array.from(boardRef.current?.querySelectorAll<HTMLElement>('[data-graph-node]') ?? []).map(
      (node) => node.dataset.graphNode!,
    );
  const placement = useGraphPlacement(view, saved, disabled, change, camera, freeze, visibleNodes);
  const arrowId = useId();
  const menu = useContextMenu();
  const options: MenuEntry[] = [
    {
      label: '重新排版',
      icon: <AppIcon name="shrink" />,
      disabled: disabled || !view.graph.positions.length,
      run: onReset,
    },
    { label: '重新读取画布', icon: <AppIcon name="refresh" />, run: onRefresh },
  ];
  const connect = (source: string, target: string) => {
    if (disabled || source === target) return;
    const graph = view.graph;
    if (!graph.sessions.includes(source) || !graph.sessions.includes(target)) return;
    if (
      graph.edges.some(
        (edge) => edge.source_session_id === source && edge.target_session_id === target,
      )
    )
      return;
    void change(
      { action: 'connect', source_session_id: source, target_session_id: target },
      graphRevision(view),
    );
  };
  const link = useGraphLink({
    board: boardRef,
    camera,
    positions: saved,
    sessions: view.graph.sessions,
    disabled,
    connect,
  });
  const keys = (event: KeyboardEvent<HTMLElement>, node?: string) => {
    if (event.defaultPrevented) return;
    if (event.key === 'Escape') {
      if (link.active || placement.box || placement.selection.size) event.preventDefault();
      panEnd();
      placement.clear();
      link.cancel();
      return;
    }
    if (!link.active && node && event.ctrlKey && event.key === ' ') {
      event.preventDefault();
      placement.toggle(node);
      return;
    }
    if (node === undefined && event.key === 'Insert' && !disabled && view.candidates.length) {
      event.preventDefault();
      const bounds = event.currentTarget.getBoundingClientRect();
      menu.openAt(
        bounds.left + 8,
        bounds.top + 8,
        '选择会话',
        view.candidates.map((candidate) => ({
          label: candidate.name,
          icon: <AppIcon name="chat" />,
          run: () => onAdd(candidate.session_id),
        })),
      );
      return;
    }
    const direction = DIRECTION[event.key];
    if (!direction) return;
    event.preventDefault();
    const first = node ?? placement.selection.values().next().value;
    if (first && !link.active) {
      const step = event.shiftKey ? 24 : 8;
      placement.nudge(first, direction[0] * step, direction[1] * step);
    } else panBy(-direction[0] * 40, -direction[1] * 40);
  };
  const { x, y, zoom } = camera;
  const box = placement.box;
  return (
    <div
      role="group"
      aria-label="Agent 画布"
      aria-keyshortcuts="Insert"
      className="agent-canvas agent-graph-canvas"
      data-linking={link.active ? 'true' : undefined}
      ref={boardRef}
      tabIndex={0}
      onPointerDown={(event) => {
        if (link.active || !placement.selectStart(event)) panStart(event);
      }}
      onPointerMove={(event) => {
        if (!link.move(event)) {
          panMove(event);
          placement.move(event);
        }
      }}
      onPointerUp={(event) => {
        panEnd();
        if (!link.release(event, true)) placement.release(true, event.ctrlKey);
      }}
      onPointerCancel={(event) => {
        panEnd();
        if (!link.release(event, false)) placement.release(false);
      }}
      onContextMenu={(event) => {
        if (event.target === event.currentTarget) menu.open(event, '画布操作', options);
      }}
      onDragOver={(event) => {
        if (!disabled && event.dataTransfer.types.includes(SESSION_DRAG)) {
          event.preventDefault();
          event.dataTransfer.dropEffect = 'copy';
        }
      }}
      onDrop={(event) => {
        if (!event.dataTransfer.types.includes(SESSION_DRAG)) return;
        event.preventDefault();
        const other = draggedSession(event.dataTransfer);
        if (!disabled && other) onAdd(other);
      }}
      onKeyDown={(event) => {
        if (event.key === 'Control') placement.modifier(event.ctrlKey);
        if (event.target === event.currentTarget) keys(event);
      }}
      onKeyUp={(event) => {
        if (event.key === 'Control') placement.modifier(event.ctrlKey);
      }}
      style={{
        backgroundPosition: `${Math.round(x)}px ${Math.round(y)}px`,
        backgroundSize: `${16 * zoom}px ${16 * zoom}px`,
      }}
    >
      <div
        className="agent-canvas-world"
        style={{ transform: `translate(${x}px, ${y}px) scale(${zoom})` }}
      >
        <GraphPeerLines
          view={view}
          positions={placement.positions}
          disabled={disabled}
          onDisconnect={(edge) => {
            if (!disabled) void change({ action: 'disconnect', ...edge }, graphRevision(view));
          }}
        />
        {view.teams.map((entry) => (
          <GraphParentNodes
            key={entry.parent_session_id}
            entry={entry}
            view={view}
            positions={placement.positions}
            selected={sessionId}
            selectedForMove={placement.selection}
            chosen={chosen}
            disabled={disabled}
            linking={link.active}
            onSelect={(node) => {
              if (!link.pick(node)) placement.select(node);
            }}
            onOpen={onPick}
            onGrab={(event, id) => {
              if (!link.grab(event, id)) placement.grab(event, id);
            }}
            onNodeKey={keys}
            onRemove={onRemove}
            onEditProfile={onEditProfile}
            onStartLink={link.start}
          />
        ))}
      </div>
      {link.arrow ? (
        <svg aria-hidden="true" className="agent-graph-link-preview">
          <defs>
            <marker id={arrowId} markerWidth="8" markerHeight="8" refX="8" refY="4" orient="auto">
              <path d="M 0 0 L 8 4 L 0 8 z" fill="currentColor" />
            </marker>
          </defs>
          <line
            x1={link.arrow.from.x}
            y1={link.arrow.from.y}
            x2={link.arrow.to.x}
            y2={link.arrow.to.y}
            stroke="currentColor"
            markerEnd={`url(#${arrowId})`}
          />
        </svg>
      ) : null}
      {box ? (
        <div
          aria-hidden="true"
          className="agent-graph-selection"
          style={{
            left: Math.min(box.from.x, box.to.x),
            top: Math.min(box.from.y, box.to.y),
            width: Math.abs(box.to.x - box.from.x),
            height: Math.abs(box.to.y - box.from.y),
          }}
        />
      ) : null}
      {placement.guides?.x !== undefined ? (
        <div
          aria-hidden="true"
          className="agent-graph-guide is-vertical"
          style={{ left: x + placement.guides.x * zoom }}
        />
      ) : null}
      {placement.guides?.y !== undefined ? (
        <div
          aria-hidden="true"
          className="agent-graph-guide is-horizontal"
          style={{ top: y + placement.guides.y * zoom }}
        />
      ) : null}
      <div className="agent-graph-tools">
        <button
          aria-haspopup="menu"
          aria-label="画布操作"
          className="button is-small"
          title="画布操作"
          type="button"
          onClick={(event) => menu.open(event, '画布操作', options)}
        >
          <AppIcon name="more" />
        </button>
        {link.active ? (
          <button className="button is-small" type="button" onClick={link.cancel}>
            <AppIcon name="close" />
            取消连线
          </button>
        ) : null}
      </div>
      <div className="agent-canvas-zoom" onPointerDown={(event) => event.stopPropagation()}>
        <button aria-label="缩小" title="缩小" type="button" onClick={() => zoomBy(1 / 1.2)}>
          <AppIcon name="minus" size={12} />
        </button>
        <output aria-label="画布缩放比例">{Math.round(zoom * 100)}%</output>
        <button aria-label="放大" title="放大" type="button" onClick={() => zoomBy(1.2)}>
          <AppIcon name="plus" size={12} />
        </button>
        <button type="button" onClick={showAll}>
          显示全部
        </button>
      </div>
      {menu.menu}
    </div>
  );
}
