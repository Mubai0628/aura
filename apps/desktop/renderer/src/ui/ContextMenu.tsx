import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from 'react';
import './context-menu.css';

export interface MenuAction {
  label: string;
  icon?: ReactNode;
  disabled?: boolean;
  /** Said under the label: why it cannot be used here, or what it will do. */
  note?: string;
  run: () => void;
}

export type MenuEntry = MenuAction | 'separator';

export interface Opened {
  x: number;
  y: number;
  label: string;
  entries: MenuEntry[];
}

/** One open menu, in the top layer (see `useContextMenu`). */
export function ContextMenu({ x, y, label, entries, onClose }: Opened & { onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  // Focus goes back where it was when the menu is left with Escape or used.
  const [origin] = useState(() =>
    document.activeElement instanceof HTMLElement ? document.activeElement : null,
  );
  const leave = useCallback(() => {
    onClose();
    origin?.focus();
  }, [onClose, origin]);

  useLayoutEffect(() => {
    const menu = ref.current;
    if (!menu) return;
    if (typeof menu.showPopover === 'function') menu.showPopover();
    else menu.removeAttribute('popover');
    const bounds = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(8, Math.min(x, window.innerWidth - bounds.width - 8))}px`;
    menu.style.top = `${Math.max(8, Math.min(y, window.innerHeight - bounds.height - 8))}px`;
    items(menu)[0]?.focus();
    const outside = (event: Event) => {
      if (!(event.target instanceof Node) || !menu.contains(event.target)) onClose();
    };
    const away = () => onClose();
    document.addEventListener('pointerdown', outside, true);
    // The wheel closes it, not any scroll: the conversation following an answer, or a tab
    // brought into view as it is right-clicked, would close the menu the moment it opened.
    document.addEventListener('wheel', outside, { capture: true, passive: true });
    window.addEventListener('resize', away);
    window.addEventListener('blur', away);
    return () => {
      document.removeEventListener('pointerdown', outside, true);
      document.removeEventListener('wheel', outside, true);
      window.removeEventListener('resize', away);
      window.removeEventListener('blur', away);
    };
  }, [x, y, onClose]);

  const keys = (event: KeyboardEvent<HTMLDivElement>) => {
    const list = items(event.currentTarget);
    const at = list.indexOf(document.activeElement as HTMLButtonElement);
    const move = (index: number) => {
      event.preventDefault();
      list[(index + list.length) % list.length]?.focus();
    };
    if (event.key === 'ArrowDown') move(at + 1);
    else if (event.key === 'ArrowUp') move(at - 1);
    else if (event.key === 'Home') move(0);
    else if (event.key === 'End') move(list.length - 1);
    else if (event.key === 'Escape') {
      event.preventDefault();
      leave();
    } else if (event.key === 'Tab') onClose();
  };

  return (
    <div
      aria-label={label}
      className="context-menu"
      onContextMenu={(event) => event.preventDefault()}
      onKeyDown={keys}
      popover="manual"
      ref={ref}
      role="menu"
    >
      {entries.map((entry, index) =>
        entry === 'separator' ? (
          <hr key={`separator-${index}`} />
        ) : (
          <button
            aria-disabled={entry.disabled || undefined}
            key={`${index}:${entry.label}`}
            onClick={() => {
              if (entry.disabled) return;
              leave();
              entry.run();
            }}
            role="menuitem"
            type="button"
          >
            <span aria-hidden="true" className="context-menu-icon">
              {entry.icon}
            </span>
            <span>{entry.label}</span>
            {entry.note ? <span className="context-menu-note">{entry.note}</span> : null}
          </button>
        ),
      )}
    </div>
  );
}

/** The items a key can move to: every item, the unavailable ones too, so their notes are read. */
const items = (menu: HTMLElement) => [
  ...menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
];
