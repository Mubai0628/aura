import type { Dispatch, RefObject, SetStateAction } from 'react';
import { useEffect, useState } from 'react';

export function useDismissablePopover(
  containerRef: RefObject<HTMLElement | null>,
): readonly [boolean, Dispatch<SetStateAction<boolean>>] {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (!open) return;

    const dismissOutside = (event: PointerEvent) => {
      if (event.target instanceof Node && !containerRef.current?.contains(event.target)) {
        setOpen(false);
      }
    };
    const dismissWithEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };

    document.addEventListener('pointerdown', dismissOutside, true);
    document.addEventListener('keydown', dismissWithEscape);
    return () => {
      document.removeEventListener('pointerdown', dismissOutside, true);
      document.removeEventListener('keydown', dismissWithEscape);
    };
  }, [containerRef, open]);

  return [open, setOpen] as const;
}
