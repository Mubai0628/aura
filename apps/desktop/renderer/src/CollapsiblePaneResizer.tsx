import type { KeyboardEvent, PointerEvent } from 'react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { setPaneDragging } from './pane-drag';

const COLLAPSE_OVERSHOOT_PX = 48;

interface DragState {
  latestWidth: number;
  pointerId: number;
  previewCollapsed: boolean;
  startWidth: number;
  startX: number;
}

interface CollapsiblePaneResizerProps {
  className: string;
  controls: string;
  direction: 'left' | 'right';
  disabled?: boolean;
  label: string;
  maximum: number;
  minimum: number;
  onCollapse: () => void;
  onDragStateChange?: (dragging: boolean) => void;
  onResize: (width: number) => void;
  onResizeCommit: (width: number) => void;
  onTemporaryCollapseChange?: (collapsed: boolean) => void;
  width: number;
}

function bounded(width: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, Math.round(width)));
}

export function CollapsiblePaneResizer({
  className,
  controls,
  direction,
  disabled = false,
  label,
  maximum,
  minimum,
  onCollapse,
  onDragStateChange,
  onResize,
  onResizeCommit,
  onTemporaryCollapseChange,
  width,
}: CollapsiblePaneResizerProps) {
  const drag = useRef<DragState | undefined>(undefined);
  const separatorRef = useRef<HTMLDivElement>(null);
  const [collapseArmed, setCollapseArmed] = useState(false);

  const finishResize = useCallback(
    (element: HTMLElement | null, releaseCapture: boolean) => {
      const active = drag.current;
      if (active === undefined) return;
      drag.current = undefined;
      if (releaseCapture && element !== null && element.hasPointerCapture(active.pointerId)) {
        element.releasePointerCapture(active.pointerId);
      }
      setCollapseArmed(false);
      onDragStateChange?.(false);
      setPaneDragging(false);
      if (active.previewCollapsed) {
        onCollapse();
        onTemporaryCollapseChange?.(false);
      } else {
        onResizeCommit(active.latestWidth);
      }
    },
    [onCollapse, onDragStateChange, onResizeCommit, onTemporaryCollapseChange],
  );

  useEffect(
    () => () => {
      if (drag.current !== undefined) setPaneDragging(false);
    },
    [],
  );

  useEffect(() => {
    const handleWindowBlur = () => finishResize(separatorRef.current, true);
    window.addEventListener('blur', handleWindowBlur);
    return () => window.removeEventListener('blur', handleWindowBlur);
  }, [finishResize]);

  const handlePointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (disabled) return;
    drag.current = {
      latestWidth: width,
      pointerId: event.pointerId,
      previewCollapsed: false,
      startWidth: width,
      startX: event.clientX,
    };
    onDragStateChange?.(true);
    setPaneDragging(true);
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const active = drag.current;
    if (active === undefined || active.pointerId !== event.pointerId) return;
    const delta = event.clientX - active.startX;
    const rawWidth = active.startWidth + (direction === 'left' ? delta : -delta);

    if (active.previewCollapsed) {
      if (rawWidth < minimum) return;
      active.previewCollapsed = false;
      active.latestWidth = minimum;
      setCollapseArmed(false);
      onTemporaryCollapseChange?.(false);
      onResize(minimum);
      return;
    }

    if (rawWidth <= minimum - COLLAPSE_OVERSHOOT_PX) {
      active.previewCollapsed = true;
      setCollapseArmed(true);
      onTemporaryCollapseChange?.(true);
      return;
    }

    setCollapseArmed(rawWidth < minimum);
    const nextWidth = bounded(rawWidth, minimum, maximum);
    active.latestWidth = nextWidth;
    onResize(nextWidth);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (disabled) return;
    if (event.key === 'Enter') {
      event.preventDefault();
      onCollapse();
      return;
    }
    const step = event.shiftKey ? 48 : 16;
    let next: number | undefined;
    if (event.key === 'Home') next = minimum;
    if (event.key === 'End') next = maximum;
    if (event.key === 'ArrowLeft') next = width + (direction === 'left' ? -step : step);
    if (event.key === 'ArrowRight') next = width + (direction === 'left' ? step : -step);
    if (next === undefined) return;
    event.preventDefault();
    const nextWidth = bounded(next, minimum, maximum);
    onResize(nextWidth);
    onResizeCommit(nextWidth);
  };

  return (
    <div
      aria-controls={controls}
      aria-disabled={disabled ? 'true' : undefined}
      aria-label={collapseArmed ? `${label}，向内拖回可重新展开，松开将关闭` : label}
      aria-orientation="vertical"
      aria-valuemax={maximum}
      aria-valuemin={minimum}
      aria-valuenow={width}
      className={`pane-resizer ${className}`}
      data-collapse-armed={collapseArmed ? 'true' : 'false'}
      onKeyDown={handleKeyDown}
      onLostPointerCapture={(event) => finishResize(event.currentTarget, false)}
      onPointerCancel={(event) => finishResize(event.currentTarget, true)}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={(event) => finishResize(event.currentTarget, true)}
      ref={separatorRef}
      role="separator"
      tabIndex={disabled ? -1 : 0}
    />
  );
}
