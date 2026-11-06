/** The parts of xterm's buffer a terminal's text is read from; tests pass a fake. */
export interface TextBuffer {
  readonly length: number;
  getLine(
    y: number,
  ): { readonly isWrapped: boolean; translateToString(trim?: boolean): string } | undefined;
}

/**
 * A terminal's lines as it shows them, its last `lines` of them: rows xterm wrapped are one line
 * again, and the empty rows below the last output are left out.
 */
export function bufferText(buffer: TextBuffer, lines: number): string {
  const out: string[] = [];
  for (let y = 0; y < buffer.length; y++) {
    const line = buffer.getLine(y);
    if (!line) continue;
    const text = line.translateToString(true);
    if (line.isWrapped && out.length) out[out.length - 1] += text;
    else out.push(text);
  }
  while (out.length && !out[out.length - 1]!.trim()) out.pop();
  return out.slice(-lines).join('\n');
}
