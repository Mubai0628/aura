import type { CommandEntry } from '@aura/contracts/generated';

/**
 * Slash rules (detailed design §23.2, frontend design §10.6). Only a `/` the user
 * really typed opens the menu: the decision uses the input event's type, never the
 * text, so pasted, dropped, undone or restored `/` characters stay plain text.
 */
export function typedSlash(event: Pick<InputEvent, 'inputType' | 'data'>): boolean {
  return event.inputType === 'insertText' && event.data === '/';
}

/** A `/` opens the menu only at the start, at a line start or after whitespace. */
export function slashAllowedAt(value: string, position: number): boolean {
  if (position === 0) return true;
  return /\s/u.test(value[position - 1] ?? '');
}

/** The text typed after the `/` so far, or undefined once the token has ended. */
export function slashQuery(value: string, start: number, caret: number): string | undefined {
  if (value[start] !== '/' || caret <= start) return undefined;
  const query = value.slice(start + 1, caret);
  return /\s/u.test(query) ? undefined : query;
}

/** The draft without the `/query` token, and where the caret goes. */
export function withoutToken(
  value: string,
  start: number,
  caret: number,
): { value: string; caret: number } {
  return { value: value.slice(0, start) + value.slice(caret), caret: start };
}

/** A prompt command's template in place of the token; `$ARGUMENTS` becomes the caret. */
export function withTemplate(
  value: string,
  start: number,
  caret: number,
  template: string,
): { value: string; caret: number } {
  const marker = template.indexOf('$ARGUMENTS');
  const text = marker < 0 ? template : template.slice(0, marker) + template.slice(marker + 10);
  return {
    value: value.slice(0, start) + text + value.slice(caret),
    caret: start + (marker < 0 ? text.length : marker),
  };
}

/** Name prefix matches first, then name or title containing the query; menu order kept. */
export function matchCommands(commands: readonly CommandEntry[], query: string): CommandEntry[] {
  const q = query.toLowerCase();
  const prefix = commands.filter((c) => c.name.startsWith(q));
  const contains = commands.filter(
    (c) => !c.name.startsWith(q) && (c.name.includes(q) || c.title.toLowerCase().includes(q)),
  );
  return [...prefix, ...contains];
}
