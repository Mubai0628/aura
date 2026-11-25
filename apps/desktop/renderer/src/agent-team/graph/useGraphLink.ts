import { useRef, useState, type PointerEvent, type RefObject } from 'react';
import { LEAD, type Camera, type Point, type Positions } from '../canvas/layout';

type Drag = { source: string; pointerId: number; from: Point; moved: boolean };
const DRAG_THRESHOLD = 6;

export function useGraphLink({
  board,
  camera,
  positions,
  sessions,
  disabled,
  connect,
}: {
  board: RefObject<HTMLDivElement | null>;
  camera: Camera;
  positions: Positions;
  sessions: readonly string[];
  disabled: boolean;
  connect: (source: string, target: string) => void;
}) {
  const [source, setSource] = useState<string>();
  const [arrow, setArrow] = useState<{ from: Point; to: Point }>();
  const drag = useRef<Drag>(undefined);
  const suppressClick = useRef(false);
  const active = source !== undefined && sessions.includes(source) ? source : undefined;
  const root = (id: string | undefined): id is string => !!id && sessions.includes(id);
  const position = ({ clientX: x, clientY: y }: PointerEvent): Point => ({ x, y });

  return {
    active,
    arrow,
    start(node: string) {
      if (!disabled && root(node)) setSource(node);
    },
    cancel() {
      drag.current = undefined;
      suppressClick.current = false;
      setArrow(undefined);
      setSource(undefined);
    },
    grab(event: PointerEvent<HTMLButtonElement>, node: string) {
      if (event.button !== 0) return false;
      suppressClick.current = false;
      if (!active && !(event.shiftKey && root(node))) return false;
      event.stopPropagation();
      if (!disabled && root(node)) {
        drag.current = {
          pointerId: event.pointerId,
          source: node,
          from: position(event),
          moved: false,
        };
        event.currentTarget.setPointerCapture?.(event.pointerId);
      }
      return true;
    },
    move(event: PointerEvent<HTMLDivElement>) {
      const current = drag.current;
      const origin = current && positions[current.source];
      const bounds = board.current?.getBoundingClientRect();
      if (!current || !origin || !bounds || current.pointerId !== event.pointerId) return false;
      const to = position(event);
      if (
        !current.moved &&
        Math.hypot(to.x - current.from.x, to.y - current.from.y) <= DRAG_THRESHOLD
      )
        return true;
      current.moved = true;
      const center = {
        x: bounds.left + camera.x + (origin.x + LEAD.width / 2) * camera.zoom,
        y: bounds.top + camera.y + (origin.y + LEAD.height / 2) * camera.zoom,
      };
      const direction = Math.atan2(to.y - center.y, to.x - center.x);
      const dx = Math.cos(direction);
      const dy = Math.sin(direction);
      const edge =
        camera.zoom * Math.min(LEAD.width / (2 * Math.abs(dx)), LEAD.height / (2 * Math.abs(dy)));
      setArrow({
        from: { x: center.x + dx * edge - bounds.left, y: center.y + dy * edge - bounds.top },
        to: { x: to.x - bounds.left, y: to.y - bounds.top },
      });
      return true;
    },
    release(event: PointerEvent<HTMLDivElement>, commit: boolean) {
      const current = drag.current;
      if (!current || current.pointerId !== event.pointerId) return false;
      drag.current = undefined;
      setArrow(undefined);
      suppressClick.current = current.moved || !commit;
      if (commit && current.moved && !disabled) {
        const hit = document.elementFromPoint?.(event.clientX, event.clientY) ?? event.target;
        const target =
          hit instanceof Element ? hit.closest<HTMLElement>('[data-graph-node]') : null;
        const id = target?.dataset.graphNode;
        if (target && board.current?.contains(target) && root(id)) {
          connect(current.source, id);
          if (active) setSource(current.source);
        }
      }
      return true;
    },
    pick(node: string) {
      if (suppressClick.current) return true;
      if (!active) return false;
      if (!disabled && root(node)) connect(active, node);
      return true;
    },
  };
}
