import { useCallback, useState, type MouseEvent } from 'react';
import { ContextMenu, type MenuEntry, type Opened } from './ContextMenu';

export type { MenuAction, MenuEntry } from './ContextMenu';

/**
 * A menu where the pointer was right-clicked, or below the element a keyboard
 * opened it from (the menu key, Shift+F10). It sits in the top layer, so no scrolling pane clips
 * it; arrows move, Enter picks, Escape or a click elsewhere closes it.
 */
export function useContextMenu() {
  const [opened, setOpened] = useState<Opened>();
  const openAt = useCallback(
    (x: number, y: number, label: string, entries: MenuEntry[]) =>
      setOpened({ x, y, label, entries }),
    [],
  );
  const open = useCallback(
    (event: MouseEvent<Element>, label: string, entries: MenuEntry[]) => {
      event.preventDefault();
      event.stopPropagation();
      let { clientX: x, clientY: y } = event;
      if (x === 0 && y === 0) {
        const bounds = event.currentTarget.getBoundingClientRect();
        x = bounds.left;
        y = bounds.bottom;
      }
      openAt(x, y, label, entries);
    },
    [openAt],
  );
  const close = useCallback(() => setOpened(undefined), []);
  const menu = opened ? <ContextMenu {...opened} onClose={close} /> : null;
  return { open, openAt, close, menu };
}
