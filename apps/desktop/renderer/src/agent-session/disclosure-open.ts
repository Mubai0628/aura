import type { SyntheticEvent } from 'react';
import { useCallback, useState } from 'react';

/** The real open state of one `<details>`.
 *
 * Direction is decided in exactly one place: the `open` boolean handed to
 * `ExpandIcon`. A `<details>` hides its state in the DOM instead of React, so
 * the native `toggle` event (fired after the attribute changed) is the only
 * truthful source — never a hardcoded `false`, and never a CSS rotation that
 * would fight the glyph. */
export function useDisclosureOpen(initialOpen = false) {
  const [open, setOpen] = useState(initialOpen);
  const onToggle = useCallback((event: SyntheticEvent<HTMLDetailsElement>) => {
    setOpen(event.currentTarget.open);
  }, []);
  return { onToggle, open };
}
