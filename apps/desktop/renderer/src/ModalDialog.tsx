import type { KeyboardEvent, MouseEvent, ReactNode, RefObject } from 'react';
import { useLayoutEffect, useRef } from 'react';
import { createPortal } from 'react-dom';

const FOCUSABLE_SELECTOR = [
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[href]',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

interface ModalDialogProps {
  children: ReactNode;
  className?: string;
  fallbackFocusRef?: RefObject<HTMLElement | null> | undefined;
  initialFocusRef?: RefObject<HTMLElement | null> | undefined;
  layer?: 'confirmation' | 'default';
  onClose: () => void;
  /** Native modal visibility. Callers that mount the dialog only while it is
   * open keep the default; a caller that stays mounted must pass its state so
   * "closed" always closes. */
  open?: boolean;
  returnFocusRef?: RefObject<HTMLElement | null> | undefined;
  suspended?: boolean;
  titleId: string;
}

const canReceiveFocus = (element: HTMLElement | null | undefined): element is HTMLElement =>
  element !== null &&
  element !== undefined &&
  element.isConnected &&
  element.closest('[inert]') === null;

/** Opens or closes the native modal layer. Closing is authoritative: a dialog
 * whose state says "closed" must never stay visible or keep the page inert. */
const setDialogOpen = (element: HTMLDialogElement, open: boolean): void => {
  try {
    if (open) {
      if (typeof element.showModal === 'function' && !element.open) element.showModal();
      else if (!element.open) element.setAttribute('open', '');
    } else if (element.open) {
      if (typeof element.close === 'function') element.close();
      else element.removeAttribute('open');
    }
  } catch {
    if (open) element.setAttribute('open', '');
    else element.removeAttribute('open');
  }
};

/** A modal layer must be unique per logical dialog. A leftover node for the
 * same dialog (a repeated mount, a reactivated portal) would otherwise stay in
 * the top layer and keep the whole page inert, so no control behind it - not
 * even its own close button - can be clicked. */
const closeStaleLayers = (element: HTMLDialogElement, titleId: string): void => {
  for (const other of document.querySelectorAll<HTMLDialogElement>('dialog.modal-backdrop')) {
    if (other === element || !other.open) continue;
    if (other.querySelector(`#${CSS.escape(titleId)}`) === null) continue;
    setDialogOpen(other, false);
  }
};

const restoreFocus = (
  returnFocusRef: RefObject<HTMLElement | null> | undefined,
  previousFocusRef: RefObject<HTMLElement | null>,
  fallbackFocusRef: RefObject<HTMLElement | null> | undefined,
): void => {
  const destination = returnFocusRef?.current ?? previousFocusRef.current;
  if (canReceiveFocus(destination)) {
    destination.focus();
  } else if (canReceiveFocus(fallbackFocusRef?.current)) {
    fallbackFocusRef.current.focus();
  }
};

export function ModalDialog({
  children,
  className = '',
  fallbackFocusRef,
  initialFocusRef,
  layer = 'default',
  onClose,
  open = true,
  returnFocusRef,
  suspended = false,
  titleId,
}: ModalDialogProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const layerRef = useRef<HTMLDialogElement>(null);
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);

  useLayoutEffect(() => {
    previouslyFocusedRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return () => {
      restoreFocus(returnFocusRef, previouslyFocusedRef, fallbackFocusRef);
    };
  }, [fallbackFocusRef, returnFocusRef]);

  useLayoutEffect(() => {
    // Use the browser's modal top layer so pane stacking and stale overlays
    // cannot intercept the close control or backdrop. Visibility is driven by
    // the caller's state, so a closed dialog can never stay on screen.
    const layerElement = layerRef.current;
    if (!layerElement) return;
    if (open) closeStaleLayers(layerElement, titleId);
    setDialogOpen(layerElement, open);
    return () => {
      // A modal must never outlive its own unmount: closing removes it from the
      // top layer, and the stylesheet keeps a closed dialog out of the hit test.
      setDialogOpen(layerElement, false);
    };
  }, [open, titleId]);

  useLayoutEffect(() => {
    const dialog = dialogRef.current;
    if (dialog === null) return;
    dialog.inert = suspended;
    if (!open || suspended) return;
    const initial = initialFocusRef?.current;
    const firstFocusable = dialog.querySelector<HTMLElement>(FOCUSABLE_SELECTOR);
    if (canReceiveFocus(initial)) initial.focus();
    else if (!dialog.contains(document.activeElement)) firstFocusable?.focus();
  }, [initialFocusRef, open, suspended]);

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (suspended) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== 'Tab') return;
    const focusable = Array.from(
      dialogRef.current?.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR) ?? [],
    ).filter((element) => !element.closest('[inert]'));
    const first = focusable[0];
    const last = focusable.at(-1);
    if (first === undefined || last === undefined) {
      event.preventDefault();
      return;
    }
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const handleBackdropMouseDown = (event: MouseEvent<HTMLElement>) => {
    if (!suspended && event.target === event.currentTarget) onClose();
  };

  // Portalled to <body> so the application root becomes inert for assistive
  // technology while the modal layer is open; showModal() still puts the layer
  // itself in the browser's top layer. Closed layers are hidden by the
  // stylesheet's `:not([open])` rule, so a leftover node cannot intercept
  // pointer events.
  return createPortal(
    <dialog
      ref={layerRef}
      onCancel={(event) => {
        event.preventDefault();
        if (!suspended) onClose();
      }}
      aria-hidden={suspended ? 'true' : undefined}
      className={`modal-backdrop modal-backdrop-${layer}`}
      onMouseDown={handleBackdropMouseDown}
      role="presentation"
    >
      <div
        aria-labelledby={titleId}
        aria-modal={suspended ? undefined : 'true'}
        className={`modal-dialog ${className}`.trim()}
        onKeyDown={handleKeyDown}
        ref={dialogRef}
        role="dialog"
      >
        {children}
      </div>
    </dialog>,
    document.body,
  );
}
