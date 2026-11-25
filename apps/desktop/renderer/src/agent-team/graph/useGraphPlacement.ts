import type { AgentGraphRevision, AgentGraphView } from '@aura/contracts/generated';
import { useRef, useState, type PointerEvent } from 'react';
import { LEAD, MEMBER, type Camera, type Point, type Positions } from '../canvas/layout';
import { positionsPayload } from './graph-layout';
import {
  snapAlignment,
  snapDisplacement,
  type SnapAlignment,
  type SnapGuides,
} from './snap-alignment';
import { graphRevision, type GraphChange } from './useAgentGraph';

type Moving = {
  from: Point;
  zoom: number;
  positions: Positions;
  revision: AgentGraphRevision;
  alignment: SnapAlignment | undefined;
  delta?: Point;
};
type SelectionBox = { from: Point; to: Point };

function moved(positions: Positions, dx: number, dy: number): Positions {
  return Object.fromEntries(
    Object.entries(positions).map(([id, point]) => [id, { x: point.x + dx, y: point.y + dy }]),
  );
}

export function useGraphPlacement(
  view: AgentGraphView,
  saved: Positions,
  disabled: boolean,
  change: GraphChange,
  camera: Camera,
  freeze: () => void,
  visibleNodes: () => string[],
) {
  const [preview, setPreview] = useState<Positions>();
  const [guides, setGuides] = useState<SnapGuides>();
  const [selection, setSelection] = useState<ReadonlySet<string>>(new Set());
  const selected = useRef<ReadonlySet<string>>(new Set());
  const updateSelection = (next: ReadonlySet<string>) => {
    selected.current = next;
    setSelection(next);
  };
  const [box, setBox] = useState<SelectionBox>();
  const selecting = useRef<SelectionBox>(undefined);
  const moving = useRef<Moving>(undefined);
  const suppressPick = useRef(false);
  const parents = view.graph.sessions;
  const group = (id: string) => {
    const ids = new Set(
      selected.current.has(id) ? visibleNodes().filter((node) => selected.current.has(node)) : [id],
    );
    for (const { parent_session_id, team } of view.teams) {
      if (ids.has(parent_session_id)) {
        for (const member of team.members) ids.add(member.session_id);
      }
    }
    return Object.fromEntries(Object.entries(saved).filter(([candidate]) => ids.has(candidate)));
  };
  const toggle = (id: string) => {
    if (disabled) return;
    const next = new Set(selected.current);
    if (!next.delete(id)) next.add(id);
    updateSelection(next);
  };
  const save = (positions: Positions, revision: AgentGraphRevision) => {
    void change({ action: 'place', positions: positionsPayload(positions) }, revision).finally(() =>
      setPreview(undefined),
    );
  };
  const client = (event: PointerEvent): Point => ({ x: event.clientX, y: event.clientY });
  const local = (event: PointerEvent<HTMLDivElement>): Point => {
    const rect = event.currentTarget.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  };
  const cardBounds = (id: string) => ({ ...saved[id]!, ...(parents.includes(id) ? LEAD : MEMBER) });
  const dragPreview = (drag: Moving, delta: Point, snap: boolean) => {
    drag.delta = delta;
    const aligned = snapDisplacement(delta, snap ? drag.alignment : undefined, drag.zoom);
    setPreview(moved(drag.positions, aligned.delta.x, aligned.delta.y));
    setGuides(aligned.guides);
  };

  return {
    selection,
    box,
    guides,
    positions: preview ? { ...saved, ...preview } : saved,
    grab(event: PointerEvent<HTMLButtonElement>, id: string) {
      if (event.button !== 0 || disabled) return;
      event.stopPropagation();
      suppressPick.current = event.ctrlKey;
      if (suppressPick.current) {
        toggle(id);
        return;
      }
      if (!selected.current.has(id)) updateSelection(new Set());
      const positions = group(id);
      const stationary = visibleNodes().filter((node) => saved[node] && !positions[node]);
      moving.current = {
        from: client(event),
        zoom: camera.zoom,
        positions,
        revision: graphRevision(view),
        alignment: saved[id]
          ? snapAlignment(cardBounds(id), stationary.map(cardBounds))
          : undefined,
      };
      event.currentTarget.setPointerCapture?.(event.pointerId);
    },
    selectStart(event: PointerEvent<HTMLDivElement>) {
      if (event.button !== 0 || event.target !== event.currentTarget) return false;
      if (!event.ctrlKey) {
        updateSelection(new Set());
        return false;
      }
      if (disabled) return true;
      event.preventDefault();
      event.currentTarget.focus();
      event.currentTarget.setPointerCapture?.(event.pointerId);
      freeze();
      const from = local(event);
      selecting.current = { from, to: from };
      setBox(selecting.current);
      return true;
    },
    move(event: PointerEvent<HTMLDivElement>) {
      const area = selecting.current;
      if (area) {
        selecting.current = { from: area.from, to: local(event) };
        setBox(selecting.current);
        return;
      }
      const start = moving.current;
      if (!start) return;
      const dx = event.clientX - start.from.x;
      const dy = event.clientY - start.from.y;
      if (!start.delta && Math.hypot(dx, dy) <= 6) return;
      if (!start.delta) freeze();
      suppressPick.current = true;
      dragPreview(
        start,
        { x: Math.round(dx / start.zoom), y: Math.round(dy / start.zoom) },
        event.ctrlKey,
      );
    },
    modifier(ctrl: boolean) {
      const drag = moving.current;
      if (drag?.delta) dragPreview(drag, drag.delta, ctrl);
    },
    release(commit: boolean, ctrl = false) {
      const area = selecting.current;
      const drag = moving.current;
      selecting.current = undefined;
      moving.current = undefined;
      setBox(undefined);
      setGuides(undefined);
      if (commit && area) {
        const left = (Math.min(area.from.x, area.to.x) - camera.x) / camera.zoom;
        const right = (Math.max(area.from.x, area.to.x) - camera.x) / camera.zoom;
        const top = (Math.min(area.from.y, area.to.y) - camera.y) / camera.zoom;
        const bottom = (Math.max(area.from.y, area.to.y) - camera.y) / camera.zoom;
        const hits = visibleNodes().filter((id) => {
          const at = saved[id];
          const size = parents.includes(id) ? LEAD : MEMBER;
          return (
            at &&
            at.x < right &&
            at.x + size.width > left &&
            at.y < bottom &&
            at.y + size.height > top
          );
        });
        updateSelection(new Set(hits));
      }
      if (commit && drag?.delta) {
        const { delta } = snapDisplacement(
          drag.delta,
          ctrl ? drag.alignment : undefined,
          drag.zoom,
        );
        if (delta.x !== 0 || delta.y !== 0) {
          const positions = moved(drag.positions, delta.x, delta.y);
          setPreview(positions);
          save(positions, drag.revision);
          return;
        }
      }
      setPreview(undefined);
    },
    clear() {
      selecting.current = undefined;
      moving.current = undefined;
      updateSelection(new Set());
      setBox(undefined);
      setPreview(undefined);
      setGuides(undefined);
    },
    nudge(id: string | undefined, dx: number, dy: number) {
      if (disabled || !id) return;
      freeze();
      save(moved(group(id), dx, dy), graphRevision(view));
    },
    toggle,
    select(id: string) {
      if (!suppressPick.current) updateSelection(new Set([id]));
      suppressPick.current = false;
    },
  };
}
