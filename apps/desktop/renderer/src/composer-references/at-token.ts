/**
 * The "@" a reference is chosen with. Only an "@" the user typed opens the list, at the start of
 * a line or after a space, as with "/": a pasted or restored "@" stays text. What is chosen goes
 * into the words as "@path", in quotes when the path has a space, and Core reads it from there
 * when the message is sent.
 */
export function typedAt(event: Pick<InputEvent, 'inputType' | 'data'>): boolean {
  return event.inputType === 'insertText' && event.data === '@';
}

/** An "@" opens the list only at the start, at a line start or after a space. */
export function atAllowedAt(value: string, position: number): boolean {
  if (position === 0) return true;
  return /\s/u.test(value[position - 1] ?? '');
}

/** What was typed after the "@" so far, or undefined once the token has ended. */
export function atQuery(value: string, start: number, caret: number): string | undefined {
  if (value[start] !== '@' || caret <= start) return undefined;
  const query = value.slice(start + 1, caret);
  return /\s/u.test(query) ? undefined : query;
}

/** How a workspace path is written after "@": in quotes when it has a space. */
export function atPath(path: string): string {
  return /\s/u.test(path) ? `@"${path}"` : `@${path}`;
}

/** The words without the "@query" token, for something chosen that is not written in them. */
export function withoutToken(
  value: string,
  start: number,
  caret: number,
): { value: string; caret: number } {
  return { value: value.slice(0, start) + value.slice(caret), caret: start };
}

/** How a program is written after "@": always in quotes, so the composer can show it as a block. */
export function atProgram(name: string): string {
  return `@"${name}"`;
}

/** The words with the "@query" token replaced by the chosen path and a space, and the caret. */
export function withPath(
  value: string,
  start: number,
  caret: number,
  path: string,
): { value: string; caret: number } {
  return withWritten(value, start, caret, atPath(path));
}

/** The words with the "@query" token replaced by the chosen program and a space, and the caret. */
export function withProgram(
  value: string,
  start: number,
  caret: number,
  name: string,
): { value: string; caret: number } {
  return withWritten(value, start, caret, atProgram(name));
}

function withWritten(
  value: string,
  start: number,
  caret: number,
  token: string,
): { value: string; caret: number } {
  const written = `${token} `;
  const rest = value.slice(caret);
  // A space already there is not doubled.
  const inserted = rest.startsWith(' ') ? written.slice(0, -1) : written;
  return {
    value: value.slice(0, start) + inserted + rest,
    caret: start + written.length,
  };
}
