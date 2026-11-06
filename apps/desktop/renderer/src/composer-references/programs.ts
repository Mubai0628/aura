import type { ComputerCatalogEntryV1 } from '@aura/contracts/generated';
import { useEffect, useState } from 'react';

import { computerControl } from '../computer/bridge';
import { catalogRead } from './program-icons';
import { knowPrograms } from './program-tokens';

const NONE: readonly ComputerCatalogEntryV1[] = [];

/** The computer's programs, read once per opening of the "@" list; none while control is off. */
export function usePrograms(open: boolean): readonly ComputerCatalogEntryV1[] {
  const [programs, setPrograms] = useState(NONE);
  useEffect(() => {
    const api = computerControl();
    if (!open || api === undefined) return;
    let live = true;
    void Promise.resolve()
      .then(() => api.catalog({}))
      .then(
        (result) => {
          if (!result.ok || !result.value.enabled) return;
          knowPrograms(result.value.apps);
          catalogRead();
          if (live) setPrograms(result.value.apps);
        },
        () => undefined,
      );
    return () => {
      live = false;
      setPrograms(NONE);
    };
  }, [open]);
  return programs;
}
