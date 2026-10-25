import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

import { AppIcon } from '../app-icons';
import './picture-preview.css';

const FIT = { scale: 1, x: 0, y: 0 };

/** An image stays in the message; this transient viewer only changes how it is shown. */
export function PicturePreview({
  src,
  name,
  onClose,
}: {
  src: string;
  name: string;
  onClose: () => void;
}) {
  const surface = useRef<HTMLDialogElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  const [view, setView] = useState(FIT);
  const drag = useRef<
    { id: number; x: number; y: number; originX: number; originY: number } | undefined
  >(undefined);
  useLayoutEffect(() => {
    const previous = document.activeElement;
    surface.current?.showModal?.();
    close.current?.focus();
    const keyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        onClose();
      } else if (event.key === 'Tab') {
        event.preventDefault();
        close.current?.focus();
      }
    };
    window.addEventListener('keydown', keyDown, true);
    return () => {
      window.removeEventListener('keydown', keyDown, true);
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, [onClose]);
  useEffect(() => {
    const element = surface.current;
    if (!element) return;
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      const rect = element.getBoundingClientRect();
      const x = event.clientX - rect.left - rect.width / 2;
      const y = event.clientY - rect.top - rect.height / 2;
      setView((current) => {
        const scale = Math.max(0.1, Math.min(8, current.scale * Math.exp(-event.deltaY * 0.002)));
        const ratio = scale / current.scale;
        return { scale, x: x - (x - current.x) * ratio, y: y - (y - current.y) * ratio };
      });
    };
    element.addEventListener('wheel', wheel, { passive: false });
    return () => element.removeEventListener('wheel', wheel);
  }, []);
  return createPortal(
    <dialog
      aria-label="图片预览"
      aria-modal="true"
      className="picture-preview"
      ref={surface}
      role="dialog"
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <img
        alt={name}
        className="picture-preview-image"
        data-pannable={view.scale > 1 || undefined}
        draggable={false}
        onDoubleClick={() => setView(FIT)}
        onPointerDown={(event) => {
          if (event.button !== 0 || view.scale <= 1) return;
          event.preventDefault();
          drag.current = {
            id: event.pointerId,
            x: event.clientX,
            y: event.clientY,
            originX: view.x,
            originY: view.y,
          };
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          const active = drag.current;
          if (active?.id !== event.pointerId) return;
          setView((current) => ({
            ...current,
            x: active.originX + event.clientX - active.x,
            y: active.originY + event.clientY - active.y,
          }));
        }}
        onPointerUp={() => {
          drag.current = undefined;
        }}
        onPointerCancel={() => {
          drag.current = undefined;
        }}
        src={src}
        style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})` }}
      />
      <button
        aria-label="关闭图片预览"
        className="picture-preview-close"
        onClick={onClose}
        ref={close}
        type="button"
      >
        <AppIcon name="close" size={20} />
      </button>
    </dialog>,
    document.body,
  );
}

/** Keyboard and pointer entry shared by composer thumbnails and sent message pictures. */
export function PreviewPicture({ src, name }: { src: string; name: string }) {
  const [open, setOpen] = useState(false);
  // The close callback is stable while the viewer is mounted, preserving its focus lifetime.
  const dismiss = useCallback(() => setOpen(false), []);
  return (
    <>
      <button
        aria-label={`预览 ${name}`}
        className="attachment-picture-open"
        onClick={() => setOpen(true)}
        type="button"
      >
        <img alt={name} src={src} />
      </button>
      {open ? <PicturePreview src={src} name={name} onClose={dismiss} /> : null}
    </>
  );
}
