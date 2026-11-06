import { useEffect, useSyncExternalStore } from 'react';

import { computerControl } from '../computer/bridge';

/** How many icons one request asks for: the first ones show while the rest are read. */
const PER_REQUEST = 40;

/** Each program's icon as an image address, once read. */
const icons = new Map<string, string>();
/** Programs asked for since the last catalog: Core answers again for one without an icon. */
const asked = new Set<string>();
const waiting: string[] = [];
const listeners = new Set<() => void>();
let reading = false;
let scheduled = false;

function changed() {
  for (const listener of listeners) listener();
}

async function readWaiting(): Promise<void> {
  const api = computerControl();
  if (reading || api === undefined) return;
  reading = true;
  try {
    while (waiting.length) {
      const ids = waiting.splice(0, PER_REQUEST);
      const result = await Promise.resolve()
        .then(() => api.icons({ app_ids: ids }))
        .catch(() => undefined);
      if (!result?.ok) {
        // Asked again the next time they are wanted.
        for (const id of [...ids, ...waiting]) asked.delete(id);
        waiting.length = 0;
        return;
      }
      for (const icon of result.value.icons)
        icons.set(icon.app_id, `data:image/png;base64,${icon.png}`);
      if (result.value.icons.length) changed();
    }
  } finally {
    reading = false;
  }
}

/**
 * Asks Core, in this order, for the icons of the programs not asked for yet. The asks of one
 * render (each line of the list asks for its own) go out together.
 */
export function wantProgramIcons(ids: readonly string[]): void {
  const fresh = ids.filter((id) => !icons.has(id) && !asked.has(id));
  if (!fresh.length) return;
  for (const id of fresh) asked.add(id);
  waiting.push(...fresh);
  if (scheduled) return;
  scheduled = true;
  queueMicrotask(() => {
    scheduled = false;
    void readWaiting();
  });
}

/** A new catalog was read: programs without an icon may be asked for again. */
export function catalogRead(): void {
  asked.clear();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The program's icon as an image address; undefined until it is read, or when it has none. */
export function useProgramIcon(appId: string | undefined): string | undefined {
  useEffect(() => {
    if (appId !== undefined) wantProgramIcons([appId]);
  }, [appId]);
  return useSyncExternalStore(subscribe, () =>
    appId === undefined ? undefined : icons.get(appId),
  );
}

export function forgetProgramIconsForTests(): void {
  icons.clear();
  asked.clear();
  waiting.length = 0;
  reading = false;
  scheduled = false;
}
