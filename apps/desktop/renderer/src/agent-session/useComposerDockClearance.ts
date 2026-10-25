import { useLayoutEffect, useState, type RefObject } from 'react';

/** Keep the transcript above the floating composer as attachments and quick menus change size. */
export function useComposerDockClearance(dockRef: RefObject<HTMLDivElement | null>) {
  const [clearance, setClearance] = useState<number>();
  useLayoutEffect(() => {
    const dock = dockRef.current;
    if (dock === null) return;
    let animationFrame: number | undefined;
    const measure = () => {
      const dockRect = dock.getBoundingClientRect();
      let visibleTop = dockRect.top;
      for (const popover of dock.querySelectorAll<HTMLElement>(
        '.attachment-menu, .composer-popover',
      )) {
        const popoverRect = popover.getBoundingClientRect();
        if (popoverRect.height > 0) visibleTop = Math.min(visibleTop, popoverRect.top);
      }
      const measured = Math.ceil(dockRect.bottom - visibleTop);
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
