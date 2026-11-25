import type { PointerEvent as ReactPointerEvent } from 'react';
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { fit, zoomAbout, type Camera, type Point, type Positions } from '../canvas/layout';

const MIN_GRAPH_ZOOM = 0.05;
const CONTROL_INSET = 32;
export function useGraphCamera(positions: Positions) {
  const boardRef = useRef<HTMLDivElement>(null);
  const [board, setBoard] = useState({ width: 0, height: 0 });
  const [own, setOwn] = useState<Camera>({ x: 0, y: 0, zoom: 1 });
  const [follow, setFollow] = useState(true);
  const fitted = useMemo(() => {
    const camera = fit(
      positions,
      { width: board.width, height: Math.max(0, board.height - 2 * CONTROL_INSET) },
      MIN_GRAPH_ZOOM,
    );
    return { ...camera, y: camera.y + CONTROL_INSET };
  }, [positions, board]);
  const camera = follow ? fitted : own;
  const latest = useRef(camera);
  const pan = useRef<{ from: Point; camera: Camera } | undefined>(undefined);
  useEffect(() => {
    latest.current = camera;
  }, [camera]);
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
  useEffect(() => {
    const element = boardRef.current;
    if (!element) return;
    const wheel = (event: WheelEvent) => {
      if (event.target instanceof Element && event.target.closest('.context-menu')) return;
      event.preventDefault();
      const now = latest.current,
        box = element.getBoundingClientRect();
      setFollow(false);
      setOwn(
        event.ctrlKey
          ? zoomAbout(
              now,
              event.deltaY < 0 ? 1.1 : 1 / 1.1,
              { x: event.clientX - box.left, y: event.clientY - box.top },
              MIN_GRAPH_ZOOM,
            )
          : { ...now, x: now.x - event.deltaX, y: now.y - event.deltaY },
      );
    };
    element.addEventListener('wheel', wheel, { passive: false });
    return () => element.removeEventListener('wheel', wheel);
  }, []);
  const freeze = () => {
    setFollow(false);
    setOwn(camera);
  };
  return {
    boardRef,
    camera,
    freeze,
    zoom: (factor: number) => {
      setFollow(false);
      setOwn(
        zoomAbout(camera, factor, { x: board.width / 2, y: board.height / 2 }, MIN_GRAPH_ZOOM),
      );
    },
    showAll: () => setFollow(true),
    panStart: (event: ReactPointerEvent<HTMLDivElement>) => {
      if (event.button !== 0 || event.target !== event.currentTarget) return;
      pan.current = { from: { x: event.clientX, y: event.clientY }, camera };
      event.currentTarget.setPointerCapture(event.pointerId);
    },
    panMove: (event: ReactPointerEvent<HTMLDivElement>) => {
      const start = pan.current;
      if (!start) return;
      setFollow(false);
      setOwn({
        ...start.camera,
        x: start.camera.x + event.clientX - start.from.x,
        y: start.camera.y + event.clientY - start.from.y,
      });
    },
    panEnd: () => {
      pan.current = undefined;
    },
    panBy: (x: number, y: number) => {
      setFollow(false);
      setOwn({ ...camera, x: camera.x + x, y: camera.y + y });
    },
  };
}
