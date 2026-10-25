import type { CommandEntry } from '@aura/contracts/generated';
import type { KeyboardEvent, RefObject } from 'react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { offered, unavailableReason, type CommandContext } from './availability';
import type { CommandOption } from './CommandMenu';
import {
  matchCommands,
  slashAllowedAt,
  slashQuery,
  typedSlash,
  withTemplate,
  withoutToken,
} from './slash';

/** `start` is the typed `/`; null when the list was opened from `+` or `/help`. */
interface MenuState {
  start: number | null;
  query: string;
  highlight: number;
}

export function useSlashCommands({
  textarea,
  draft,
  setDraft,
  commands,
  context,
  onImmediate,
  onOpen,
}: {
  textarea: RefObject<HTMLTextAreaElement | null>;
  draft: string;
  setDraft: (text: string) => void;
  commands: readonly CommandEntry[];
  context: CommandContext;
  onImmediate: (command: CommandEntry) => void;
  /** Called from the event that opens the list. */
  onOpen?: () => void;
}) {
  const [menu, setMenu] = useState<MenuState>();
  const [block, setBlock] = useState<CommandEntry>();
  const typed = useRef<number | null>(null);
  const caretAfter = useRef<number | null>(null);

  useEffect(() => {
    const element = textarea.current;
    if (!element) return;
    const beforeInput = (event: InputEvent) => {
      const at = element.selectionStart;
      typed.current =
        typedSlash(event) && at === element.selectionEnd && slashAllowedAt(element.value, at)
          ? at
          : null;
    };
    element.addEventListener('beforeinput', beforeInput);
    return () => element.removeEventListener('beforeinput', beforeInput);
  }, [textarea]);

  // A pick changes the draft; the caret goes where the command text was.
  useEffect(() => {
    const caret = caretAfter.current;
    if (caret === null) return;
    caretAfter.current = null;
    textarea.current?.focus();
    textarea.current?.setSelectionRange(caret, caret);
  }, [draft, textarea]);

  const options: CommandOption[] = useMemo(() => {
    if (!menu) return [];
    const available = offered(commands);
    const listed = menu.start === null ? available : matchCommands(available, menu.query);
    return listed.map((command) => ({ command, reason: unavailableReason(command, context) }));
  }, [menu, commands, context]);

  /** After every edit or caret move: open on a typed `/`, follow the token, or close. */
  const sync = (value: string, caret: number) => {
    const start = typed.current;
    typed.current = null;
    if (start !== null && value[start] === '/' && caret === start + 1) {
      setMenu({ start, query: '', highlight: 0 });
      onOpen?.();
      return;
    }
    if (menu === undefined || menu.start === null) return;
    const query = slashQuery(value, menu.start, caret);
    if (query === undefined) setMenu(undefined);
    else if (query !== menu.query) setMenu({ ...menu, query, highlight: 0 });
  };

  const pick = (option: CommandOption) => {
    if (option.reason !== undefined) return;
    const caret = textarea.current?.selectionStart ?? draft.length;
    const start = menu?.start ?? null;
    setMenu(undefined);
    const { command } = option;
    if (command.kind === 'prompt') {
      const next = withTemplate(draft, start ?? caret, caret, command.template ?? '');
      caretAfter.current = next.caret;
      setDraft(next.value);
      return;
    }
    if (start !== null) {
      const next = withoutToken(draft, start, caret);
      caretAfter.current = next.caret;
      setDraft(next.value);
    }
    if (command.kind === 'block') setBlock(command);
    else onImmediate(command);
  };

  /** True when the key belonged to the command UI and must not also send or type. */
  const keyDown = (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    const target = event.currentTarget;
    if (
      block &&
      !menu &&
      event.key === 'Backspace' &&
      target.selectionStart === 0 &&
      target.selectionEnd === 0
    ) {
      event.preventDefault();
      setBlock(undefined);
      return true;
    }
    if (!menu || event.nativeEvent.isComposing) return false;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const count = options.length;
      if (count)
        setMenu({
          ...menu,
          highlight: (menu.highlight + (event.key === 'ArrowDown' ? 1 : count - 1)) % count,
        });
      return true;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      setMenu(undefined);
      return true;
    }
    if (event.key === 'Enter' || event.key === 'Tab') {
      const option = options[menu.highlight];
      // Nothing matches: the typed text is ordinary text, and Enter sends it as typed.
      if (option === undefined) {
        setMenu(undefined);
        return false;
      }
      event.preventDefault();
      pick(option);
      return true;
    }
    return false;
  };

  return {
    menu,
    options,
    block,
    setBlock,
    sync,
    pick,
    keyDown,
    hover: (index: number) => setMenu((current) => current && { ...current, highlight: index }),
    browse: () => {
      setMenu({ start: null, query: '', highlight: 0 });
      onOpen?.();
    },
    close: () => setMenu(undefined),
  };
}
