import { useLayoutEffect, useState, type RefObject } from 'react';

import { browserApi } from './bridge';

/** Renderer layers that, drawn over the page area, would hide under the native page view. */
const COVERING =
  '[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"], .notification-toast';

/** How long the page waits for its picture before stepping aside without one. */
const PICTURE_WAIT_MS = 150;
/** How often the picture is taken again while the page stays covered: it keeps running. */
const PICTURE_REFRESH_MS = 2_000;

const overlaps = (a: DOMRect, b: DOMRect) =>
  a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;

const clips = (style: CSSStyleDeclaration) =>
  [style.overflow, style.overflowX, style.overflowY].some(
    (value) => value !== '' && value !== 'visible',
  );

/**
 * The part of the area its panel shows. The page is
 * drawn over the window, not inside the panel, so an area wider than its panel would cover the
 * conversation beside it.
 */
function shownRect(element: HTMLElement): DOMRect {
  let { left, top, right, bottom } = element.getBoundingClientRect();
  for (let ancestor = element.parentElement; ancestor; ancestor = ancestor.parentElement) {
    if (!clips(getComputedStyle(ancestor))) continue;
    const clip = ancestor.getBoundingClientRect();
    left = Math.max(left, clip.left);
    top = Math.max(top, clip.top);
    right = Math.min(right, clip.right);
    bottom = Math.min(bottom, clip.bottom);
  }
  return new DOMRect(left, top, Math.max(0, right - left), Math.max(0, bottom - top));
}

/**
 * The page itself is a native view Main lays over the page area. While `showing`, it is kept
 * on that area, out of the way of dialogs and menus drawn over it; otherwise it is hidden.
 * Returns a picture of the page to draw in the area while a dialog or menu covers it.
 */
export function useNativeView(
  area: RefObject<HTMLDivElement | null>,
  sessionId: string,
  pageId: string,
  showing: boolean,
): string | undefined {
  const [picture, setPicture] = useState<string>();
  useLayoutEffect(() => {
    const api = browserApi();
    const element = area.current;
    if (api === undefined || element === null || !showing) return;
    let pending: (() => void) | undefined;
    let shownAt = '';
    let live = true;
    // Which covering a step aside belongs to: one that outlasted its menu does nothing.
    let coverings = 0;
    let refresh: ReturnType<typeof setInterval> | undefined;
    const picturing = () => {
      const taken = api.command({ kind: 'picture', sessionId, pageId });
      void taken.then((result) => {
        if (live && shownAt === 'covered' && result.ok && result.picture !== undefined)
          setPicture(result.picture);
      });
      return taken;
    };
    // The page stays in place until its picture is ready, briefly, so the area is never empty;
    // the menu may be gone by then.
    const stopRefresh = () => {
      if (refresh !== undefined) clearInterval(refresh);
      refresh = undefined;
    };
    const stepAside = async (covering: number) => {
      const taken = picturing();
      await Promise.race([taken, new Promise((done) => setTimeout(done, PICTURE_WAIT_MS))]);
      if (!live || shownAt !== 'covered' || covering !== coverings) return;
      void api.command({ kind: 'hide', sessionId, pageId });
      stopRefresh();
      refresh = setInterval(() => void picturing(), PICTURE_REFRESH_MS);
    };
    const place = () => {
      pending = undefined;
      const rect = shownRect(element);
      const covered = [...document.querySelectorAll(COVERING)].some(
        (layer) => !element.contains(layer) && overlaps(layer.getBoundingClientRect(), rect),
      );
      if (covered && rect.width >= 1 && rect.height >= 1) {
        if (shownAt === 'covered') return;
        // A page not shown yet has nothing to picture; it waits hidden.
        const wasShown = shownAt !== '' && shownAt !== 'hidden';
        shownAt = 'covered';
        coverings += 1;
        if (wasShown) void stepAside(coverings);
        return;
      }
      stopRefresh();
      if (covered || rect.width < 1 || rect.height < 1) {
        if (shownAt !== 'hidden') void api.command({ kind: 'hide', sessionId, pageId });
        shownAt = 'hidden';
        setPicture(undefined);
        return;
      }
      const bounds = {
        x: Math.max(0, Math.round(rect.left)),
        y: Math.max(0, Math.round(rect.top)),
        width: Math.round(rect.width),
        height: Math.round(rect.height),
      };
      const at = JSON.stringify(bounds);
      if (at === shownAt) return;
      shownAt = at;
      // The picture goes once the page is back over it.
      void api.command({ kind: 'show', sessionId, pageId, bounds }).then(() => {
        if (live && shownAt === at) setPicture(undefined);
      });
    };
    // Once per frame, however many changes arrive in it.
    const schedule = () => {
      if (pending !== undefined) return;
      if (typeof requestAnimationFrame === 'function') {
        const id = requestAnimationFrame(place);
        pending = () => cancelAnimationFrame(id);
      } else {
        const id = setTimeout(place, 16);
        pending = () => clearTimeout(id);
      }
    };
    place();
    const resize = typeof ResizeObserver === 'function' ? new ResizeObserver(schedule) : undefined;
    resize?.observe(element);
    const layers = new MutationObserver(schedule);
    layers.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['role', 'class'],
    });
    // A divider dragged, or a panel sliding, can move the area without resizing it.
    const dragging = (event: PointerEvent) => {
      if (event.buttons !== 0) schedule();
    };
    window.addEventListener('resize', schedule);
    window.addEventListener('scroll', schedule, true);
    window.addEventListener('pointermove', dragging, { capture: true, passive: true });
    window.addEventListener('pointerup', schedule, true);
    document.addEventListener('transitionend', schedule, true);
    return () => {
      live = false;
      pending?.();
      stopRefresh();
      setPicture(undefined);
      resize?.disconnect();
      layers.disconnect();
      window.removeEventListener('resize', schedule);
      window.removeEventListener('scroll', schedule, true);
      window.removeEventListener('pointermove', dragging, true);
      window.removeEventListener('pointerup', schedule, true);
      document.removeEventListener('transitionend', schedule, true);
      void api.command({ kind: 'hide', sessionId, pageId });
    };
  }, [area, showing, sessionId, pageId]);
  return picture;
}
