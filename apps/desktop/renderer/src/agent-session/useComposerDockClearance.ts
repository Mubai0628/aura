import { useLayoutEffect, useState, type RefObject } from 'react';

/**
 * Keep the transcript above the floating composer as its attachments change its size. A menu
 * opened from the composer floats over the transcript and leaves this room as it is, so a
 * transcript scrolled to its end stays where it is.
 */
export function useComposerDockClearance(dockRef: RefObject<HTMLDivElement | null>) {
  const [clearance, setClearance] = useState<number>();
  useLayoutEffect(() => {
    const dock = dockRef.current;
    if (dock === null) return;
    let animationFrame: number | undefined;
    const measure = () => {
      const dockRect = dock.getBoundingClientRect();
      const measured = Math.ceil(dockRect.bottom - dockRect.top);
      if (measured > 0) setClearance((current) => (current === measured ? current : measured));
    };
    const scheduleMeasurement = () => {
      if (animationFrame !== undefined) window.cancelAnimationFrame(animationFrame);
      animationFrame = window.requestAnimationFrame(measure);
    };
    measure();
    const resizeObserver =
      typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(scheduleMeasurement);
    resizeObserver?.observe(dock, { box: 'border-box' });
    const mutationObserver =
      typeof MutationObserver === 'undefined'
        ? undefined
        : new MutationObserver(scheduleMeasurement);
    mutationObserver?.observe(dock, { childList: true, subtree: true });
    window.addEventListener('resize', scheduleMeasurement);
    return () => {
      if (animationFrame !== undefined) window.cancelAnimationFrame(animationFrame);
      resizeObserver?.disconnect();
      mutationObserver?.disconnect();
      window.removeEventListener('resize', scheduleMeasurement);
    };
  }, [dockRef]);
  return clearance;
}
