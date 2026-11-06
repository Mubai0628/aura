import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useSyncExternalStore,
  type KeyboardEvent,
  type RefObject,
} from 'react';

import { computerControl } from '../computer/bridge';

/** How long a catalog read for the words' programs is reused. */
const LOADED_FOR_MS = 30_000;

/** The computer's programs by name, as the last catalog and the "@" list's picks know them. */
const known = new Map<string, string>();
const listeners = new Set<() => void>();
let version = 0;
let loadedAt = Number.NEGATIVE_INFINITY;

/** A program chosen with "@", as the words hold it: `@"name"`. */
export interface ProgramToken {
  start: number;
  end: number;
  name: string;
  appId: string;
}

/** Remembers the programs' names, so that the words can show each chosen one as a block. */
export function knowPrograms(programs: readonly { app_id: string; name: string }[]): void {
  let added = false;
  for (const program of programs) {
    if (known.get(program.name) === program.app_id) continue;
    known.set(program.name, program.app_id);
    added = true;
  }
  if (!added) return;
  version += 1;
  for (const listener of listeners) listener();
}

const QUOTED = /@"([^"\n]+)"/gu;

/** Each `@"name"` at a line's start or after a space, with the name and the "@" where it is. */
export function quoted(value: string): { start: number; end: number; name: string }[] {
  return [...value.matchAll(QUOTED)].flatMap((match) => {
    const start = match.index;
    if (start > 0 && !/\s/u.test(value[start - 1] ?? '')) return [];
    return [{ start, end: start + match[0].length, name: match[1] ?? '' }];
  });
}

/** The programs chosen with "@" in the words: a quoted name of a known program. */
export function programTokens(
  value: string,
  names: ReadonlyMap<string, string> = known,
): ProgramToken[] {
  return quoted(value).flatMap((token) => {
    const appId = names.get(token.name);
    return appId === undefined ? [] : [{ ...token, appId }];
  });
}

/** Reads the catalog for names the words quote that no program list has given yet. */
function loadPrograms() {
  const api = computerControl();
  if (api === undefined || Date.now() - loadedAt < LOADED_FOR_MS) return;
  loadedAt = Date.now();
  void Promise.resolve()
    .then(() => api.catalog({}))
    .then(
      (result) => {
        if (result.ok && result.value.enabled) knowPrograms(result.value.apps);
      },
      () => undefined,
    );
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * The program a sent message's `@"name"` names: undefined until a catalog or the "@" list has
 * given that name, and for a name no program has.
 */
export function useKnownProgram(name: string): string | undefined {
  const appId = useSyncExternalStore(subscribe, () => known.get(name));
  useEffect(() => {
    if (appId === undefined) loadPrograms();
  }, [appId]);
  return appId;
}

/**
 * The words' program blocks, and the keys that treat a block as one piece: Backspace just after
 * it or Delete just before it removes the whole block.
 */
export function useProgramBlocks({
  textarea,
  draft,
  setDraft,
}: {
  textarea: RefObject<HTMLTextAreaElement | null>;
  draft: string;
  setDraft: (value: string) => void;
}) {
  const seen = useSyncExternalStore(subscribe, () => version);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `seen` is the known names' revision.
  const tokens = useMemo(() => programTokens(draft), [draft, seen]);
  const unknown = useMemo(
    () => quoted(draft).some((token) => !known.has(token.name)),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `seen` is the known names' revision.
    [draft, seen],
  );
  useEffect(() => {
    if (unknown) loadPrograms();
  }, [unknown]);
  const caretAfter = useRef<number | null>(null);
  useLayoutEffect(() => {
    const caret = caretAfter.current;
    if (caret === null || !textarea.current) return;
    caretAfter.current = null;
    textarea.current.setSelectionRange(caret, caret);
  }, [draft, textarea]);
  const keyDown = (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (event.key !== 'Backspace' && event.key !== 'Delete') return false;
    const { selectionStart, selectionEnd } = event.currentTarget;
    if (selectionStart !== selectionEnd || event.nativeEvent.isComposing) return false;
    const token = tokens.find((one) =>
      event.key === 'Backspace' ? one.end === selectionStart : one.start === selectionStart,
    );
    if (token === undefined) return false;
    event.preventDefault();
    caretAfter.current = token.start;
    setDraft(draft.slice(0, token.start) + draft.slice(token.end));
    return true;
  };
  return { tokens, keyDown };
}

export function forgetProgramsForTests(): void {
  known.clear();
  loadedAt = Number.NEGATIVE_INFINITY;
  version += 1;
}
