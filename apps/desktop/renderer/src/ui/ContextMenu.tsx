import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from 'react';
import './context-menu.css';

interface MenuLabel {
  label: string;
  icon?: ReactNode;
  disabled?: boolean;
  /** Said under the label: why it cannot be used here, or what it will do. */
  note?: string;
}

export interface MenuAction extends MenuLabel {
  run: () => void;
}

type MenuBranch = MenuLabel & { entries: MenuEntry[] };
export type MenuEntry = MenuAction | MenuBranch | 'separator';

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
    place(menu, x, y);
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

  return (
    <MenuLevel menuRef={ref} label={label} entries={entries} leave={leave} onClose={onClose} />
  );
}

const EDGE = 8;

function place(menu: HTMLElement, x: number, y: number, anchor?: HTMLElement) {
  const bounds = menu.getBoundingClientRect();
  const clamp = (start: number, extent: number, viewport: number) =>
    Math.max(EDGE, Math.min(start, viewport - extent - EDGE));
  const parent = anchor?.closest('[role="menu"]')?.getBoundingClientRect();
  const top = clamp(y, bounds.height, window.innerHeight);
  const left = clamp(x, bounds.width, window.innerWidth);
  menu.style.left = `${left - (parent?.left ?? 0)}px`;
  menu.style.top = `${top - (parent?.top ?? 0)}px`;
}

function Submenu({
  branch,
  anchor,
  focus,
  back,
  leave,
  onClose,
}: {
  branch: MenuBranch;
  anchor: HTMLButtonElement;
  focus: boolean;
  back: () => void;
  leave: () => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const menu = ref.current;
    if (!menu) return;
    const at = anchor.getBoundingClientRect();
    const width = menu.getBoundingClientRect().width;
    const parent = anchor.closest('[role="menu"]')?.getBoundingClientRect() ?? at;
    const right = parent.right - 1;
    const left = parent.left - width + 1;
    const flip = right + width > window.innerWidth - EDGE && left >= EDGE;
    place(menu, flip ? left : right, at.top, anchor);
    if (focus) items(menu)[0]?.focus();
  }, [anchor, focus]);
  return (
    <MenuLevel
      menuRef={ref}
      label={branch.label}
      entries={branch.entries}
      leave={leave}
      onClose={onClose}
      back={back}
    />
  );
}

function MenuLevel({
  menuRef,
  label,
  entries,
  leave,
  onClose,
  back,
}: {
  menuRef: React.RefObject<HTMLDivElement | null>;
  label: string;
  entries: MenuEntry[];
  leave: () => void;
  onClose: () => void;
  back?: () => void;
}) {
  const [submenu, setSubmenu] = useState<{
    entry: MenuBranch;
    anchor: HTMLButtonElement;
    focus: boolean;
  }>();
  function open(entry: MenuEntry, anchor: HTMLButtonElement, focus: boolean) {
    if (entry !== 'separator' && 'entries' in entry && !entry.disabled) {
      setSubmenu({ entry, anchor, focus });
    } else if (!focus) setSubmenu(undefined);
  }
  const keys = (event: KeyboardEvent<HTMLDivElement>) => {
    if ((event.target as HTMLElement).closest('[role="menu"]') !== event.currentTarget) return;
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
    else if (event.key === 'ArrowRight' && list[at]) {
      const anchor = list[at];
      const entry = entries[Number(anchor.dataset.entry)];
      if (entry && entry !== 'separator' && 'entries' in entry) {
        event.preventDefault();
        open(entry, anchor, true);
      }
    } else if (event.key === 'ArrowLeft' && back) {
      event.preventDefault();
      back();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      (back ?? leave)();
    } else if (event.key === 'Tab') onClose();
  };

  return (
    <div
      aria-label={label}
      className={back ? 'context-menu context-submenu' : 'context-menu'}
      onContextMenu={(event) => event.preventDefault()}
      onKeyDown={keys}
      popover={back ? undefined : 'manual'}
      ref={menuRef}
      role="menu"
    >
      <div className="context-menu-items">
        {entries.map((entry, index) =>
          entry === 'separator' ? (
            <hr key={`separator-${index}`} />
          ) : (
            <button
              aria-disabled={entry.disabled || undefined}
              aria-haspopup={'entries' in entry ? 'menu' : undefined}
              aria-expanded={'entries' in entry ? submenu?.entry === entry : undefined}
              data-entry={index}
              key={`${index}:${entry.label}`}
              onPointerEnter={(event) => {
                if (submenu?.entry !== entry) open(entry, event.currentTarget, false);
              }}
              onClick={(event) => {
                if (entry.disabled) return;
                if ('entries' in entry) open(entry, event.currentTarget, true);
                else {
                  leave();
                  entry.run();
                }
              }}
              role="menuitem"
              type="button"
            >
              <span aria-hidden="true" className="context-menu-icon">
                {entry.icon}
              </span>
              <span>{entry.label}</span>
              {'entries' in entry ? (
                <span className="context-menu-chevron" aria-hidden="true">
                  ›
                </span>
              ) : null}
              {entry.note ? <span className="context-menu-note">{entry.note}</span> : null}
            </button>
          ),
        )}
      </div>
      {submenu ? (
        <Submenu
          key={submenu.entry.label}
          branch={submenu.entry}
          anchor={submenu.anchor}
          focus={submenu.focus}
          leave={leave}
          onClose={onClose}
          back={() => {
            submenu.anchor.focus();
            setSubmenu(undefined);
          }}
        />
      ) : null}
    </div>
  );
}

/** The items a key can move to: every item, the unavailable ones too, so their notes are read. */
const items = (menu: HTMLElement) => [
  ...menu.querySelectorAll<HTMLButtonElement>(':scope > .context-menu-items > [role="menuitem"]'),
];
