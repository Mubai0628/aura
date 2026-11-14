import type { AgentTeamView } from '@aura/contracts/generated';
import type { PointerEvent as ReactPointerEvent } from 'react';
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { AppIcon } from '../../app-icons';
import { useContextMenu } from '../../ui/useContextMenu';
import { useMemberMenu } from '../member-menu';
import { CanvasNode } from './CanvasNode';
import { useTeamFlows } from './flows';
import {
  LEAD_KEY,
  defaultLayout,
  fit,
  labelPoint,
  pathOf,
  route,
  zoomAbout,
  type Camera,
  type Point,
} from './layout';
import { leadFacts, memberFacts } from './node-facts';
import { place, unplaceAll, usePlaces } from './places';
import './agent-team-canvas.css';

type Drag =
  | { kind: 'pan'; from: Point; camera: Camera }
  | { kind: 'node'; key: string; from: Point; at: Point; zoom: number };

/** A pointer that moves further than this moves a card instead of picking it: a hand that shakes
 *  a little while it clicks still picks. */
const MOVE_PX = 6;

/**
 * The conversation's agents on an endless canvas: the main agent above its sub-agents, a line from
 * it to each, and what passes along a line lit while it passes. Picking an agent shows it in the
 * main area. Empty canvas pans, a wheel pans and with Ctrl zooms, an agent can be moved; until
 * the user moves the view it keeps every agent in sight as the board and the team change.
 */
export function AgentTeamCanvas({
  sessionId,
  view,
  chosen,
  onPick,
}: {
  sessionId: string;
  view: AgentTeamView | undefined;
  chosen: string | undefined;
  onPick: (agent: string | undefined) => void;
}) {
  const boardRef = useRef<HTMLDivElement>(null);
  const [board, setBoard] = useState({ width: 0, height: 0 });
  // The camera the user set; while following, the one that shows every agent is used instead.
  const [own, setOwn] = useState<Camera>({ x: 0, y: 0, zoom: 1 });
  const [follow, setFollow] = useState(true);
  const drag = useRef<Drag | undefined>(undefined);
  const moved = useRef(false);
  const members = useMemo(() => view?.members ?? [], [view]);
  const placed = usePlaces(sessionId);
  const flows = useTeamFlows(view);
  const menu = useContextMenu();
  const actions = useMemberMenu(sessionId);
  const agents = members.map((member) => member.agent).join('\n');
  const positions = useMemo(
    () => ({ ...defaultLayout(agents ? agents.split('\n') : []), ...placed }),
    [agents, placed],
  );
  const fitted = useMemo(
    () => (board.width > 0 && board.height > 0 ? fit(positions, board) : undefined),
    [positions, board],
  );
  const camera = follow && fitted ? fitted : own;
  // What a wheel, which is handled outside React, starts from.
  const shown = useRef({ camera, following: follow });
  useEffect(() => {
    shown.current = { camera, following: follow };
  });

  useLayoutEffect(() => {
    const element = boardRef.current;
    if (!element) return;
    const measure = () => setBoard({ width: element.clientWidth, height: element.clientHeight });
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  // A wheel over the canvas is the canvas's own: React's wheel listener cannot keep Ctrl+wheel
  // from zooming the whole window.
  useEffect(() => {
    const element = boardRef.current;
    if (!element) return;
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      const start = shown.current.following ? shown.current.camera : undefined;
      shown.current.following = false;
      setFollow(false);
      const box = element.getBoundingClientRect();
      const at = { x: event.clientX - box.left, y: event.clientY - box.top };
      setOwn((stored) => {
        const now = start ?? stored;
        return event.ctrlKey
          ? zoomAbout(now, event.deltaY < 0 ? 1.1 : 1 / 1.1, at)
          : { ...now, x: now.x - event.deltaX, y: now.y - event.deltaY };
      });
    };
    element.addEventListener('wheel', wheel, { passive: false });
    return () => element.removeEventListener('wheel', wheel);
  }, []);

  const zoomBy = (factor: number) => {
    setFollow(false);
    setOwn(zoomAbout(camera, factor, { x: board.width / 2, y: board.height / 2 }));
  };
  const panFrom = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || event.target !== event.currentTarget) return;
    drag.current = { kind: 'pan', from: { x: event.clientX, y: event.clientY }, camera };
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const grab = (event: ReactPointerEvent<HTMLButtonElement>, key: string) => {
    if (event.button !== 0) return;
    event.stopPropagation();
    moved.current = false;
    drag.current = {
      kind: 'node',
      key,
      from: { x: event.clientX, y: event.clientY },
      at: positions[key] ?? { x: 0, y: 0 },
      zoom: camera.zoom,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const move = (event: ReactPointerEvent<HTMLDivElement>) => {
    const now = drag.current;
    if (!now) return;
    const dx = event.clientX - now.from.x;
    const dy = event.clientY - now.from.y;
    if (now.kind === 'pan') {
      setFollow(false);
      setOwn({ ...now.camera, x: now.camera.x + dx, y: now.camera.y + dy });
      return;
    }
    if (!moved.current && Math.hypot(dx, dy) <= MOVE_PX) return;
    if (!moved.current) {
      // The view holds still while a card moves.
      setOwn(camera);
      setFollow(false);
    }
    moved.current = true;
    place(sessionId, now.key, {
      x: Math.round(now.at.x + dx / now.zoom),
      y: Math.round(now.at.y + dy / now.zoom),
    });
  };
  const release = () => {
    drag.current = undefined;
  };
  const pick = (agent: string | undefined) => {
    // A card that was moved is not picked.
    if (moved.current) {
      moved.current = false;
      return;
    }
    onPick(agent);
  };

  const lines = members.map((member) => {
    const corners = route(positions, member.agent);
    const facts = memberFacts(member);
    return { member, corners, flow: flows.get(member.agent), facts };
  });
  return (
    <div
      aria-label="Agent 画布"
      className="agent-canvas"
      onPointerCancel={release}
      onPointerDown={panFrom}
      onPointerMove={move}
      onPointerUp={release}
      ref={boardRef}
      role="group"
      style={{
        backgroundPosition: `${Math.round(camera.x)}px ${Math.round(camera.y)}px`,
        backgroundSize: `${16 * camera.zoom}px ${16 * camera.zoom}px`,
      }}
    >
      <div
        className="agent-canvas-world"
        style={{ transform: `translate(${camera.x}px, ${camera.y}px) scale(${camera.zoom})` }}
      >
        <svg aria-hidden="true" className="agent-canvas-lines" height="1" width="1">
          {lines.map(({ member, corners, flow, facts }) => {
            const flowing = flow ? pathOf(flow.up ? [...corners].reverse() : corners) : '';
            return (
              <g key={member.session_id}>
                <path
                  className="agent-canvas-line-path"
                  data-agent={member.agent}
                  data-chosen={member.agent === chosen ? 'true' : undefined}
                  data-ended={facts.ended ? 'true' : undefined}
                  d={pathOf(corners)}
                />
                {flow ? (
                  <>
                    <path className="agent-canvas-glow" data-kind={flow.kind} d={flowing} />
                    <path
                      className="agent-canvas-flow"
                      data-agent={member.agent}
                      data-kind={flow.kind}
                      d={flowing}
                    />
                  </>
                ) : null}
              </g>
            );
          })}
        </svg>
        <CanvasNode
          at={positions[LEAD_KEY] ?? { x: 0, y: 0 }}
          chosen={chosen === undefined}
          facts={leadFacts(view?.lead)}
          lead
          onPick={() => pick(undefined)}
          onPointerDown={(event) => grab(event, LEAD_KEY)}
        />
        {lines.map(({ member, facts }) => (
          <CanvasNode
            at={positions[member.agent] ?? { x: 0, y: 0 }}
            chosen={member.agent === chosen}
            facts={facts}
            key={member.session_id}
            onMenu={(event) => menu.open(event, member.name, actions.entries(member))}
            onPick={() => pick(member.agent)}
            onPointerDown={(event) => grab(event, member.agent)}
          />
        ))}
        {lines.map(({ member, corners, flow }) => {
          if (!flow) return null;
          const at = labelPoint(corners);
          return (
            <span
              className="agent-canvas-chip"
              data-kind={flow.kind}
              key={member.session_id}
              style={{ left: at.x, top: at.y }}
            >
              {flow.label}
            </span>
          );
        })}
      </div>
      {actions.problem ? (
        <p
          className="agent-canvas-problem"
          onPointerDown={(event) => event.stopPropagation()}
          role="alert"
        >
          {actions.problem}
          <button aria-label="关闭" onClick={actions.clearProblem} type="button">
            <AppIcon name="close" size={12} />
          </button>
        </p>
      ) : null}
      {menu.menu}
      {actions.dialog}
      <div className="agent-canvas-zoom" onPointerDown={(event) => event.stopPropagation()}>
        <button aria-label="缩小" onClick={() => zoomBy(1 / 1.2)} title="缩小" type="button">
          <AppIcon name="minus" size={12} />
        </button>
        <span>{Math.round(camera.zoom * 100)}%</span>
        <button aria-label="放大" onClick={() => zoomBy(1.2)} title="放大" type="button">
          <AppIcon name="plus" size={12} />
        </button>
        <button
          aria-label="全部显示"
          aria-pressed={follow}
          onClick={() => setFollow(true)}
          title="全部显示"
          type="button"
        >
          <AppIcon name="expand" size={12} />
        </button>
        <button
          aria-label="重新排列"
          onClick={() => {
            unplaceAll(sessionId);
            setFollow(true);
          }}
          title="重新排列"
          type="button"
        >
          <AppIcon name="refresh" size={12} />
        </button>
      </div>
    </div>
  );
}
