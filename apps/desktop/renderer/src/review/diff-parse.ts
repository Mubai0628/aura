/**
 * A file's unified diff as rows to show — line numbers on both sides, each
 * row's change, the pairing a side-by-side view needs, and the words that changed within a
 * changed line.
 */

export type DiffRowKind = 'context' | 'add' | 'delete' | 'note';

export interface DiffRow {
  kind: DiffRowKind;
  text: string;
  /** The line's number in the old file and in the new file, where it has one. */
  old?: number;
  new?: number;
}

export interface DiffHunk {
  /** The `@@ … @@` line. */
  header: string;
  rows: DiffRow[];
}

export interface ParsedDiff {
  hunks: DiffHunk[];
  binary: boolean;
  added: boolean;
  deleted: boolean;
}

const HUNK = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/u;

export function parseDiff(patch: string): ParsedDiff {
  const parsed: ParsedDiff = { hunks: [], binary: false, added: false, deleted: false };
  let hunk: DiffHunk | undefined;
  let oldLine = 0;
  let newLine = 0;
  for (const raw of patch.split('\n')) {
    const line = raw.endsWith('\r') ? raw.slice(0, -1) : raw;
    const header = HUNK.exec(line);
    if (header) {
      oldLine = Number(header[1]);
      newLine = Number(header[2]);
      hunk = { header: line, rows: [] };
      parsed.hunks.push(hunk);
      continue;
    }
    if (line.startsWith('diff --git ')) {
      hunk = undefined;
      continue;
    }
    if (!hunk) {
      if (line.startsWith('Binary files ') || line === 'GIT binary patch') parsed.binary = true;
      else if (line.startsWith('new file mode')) parsed.added = true;
      else if (line.startsWith('deleted file mode')) parsed.deleted = true;
      continue;
    }
    const sign = line[0];
    const text = line.slice(1);
    if (sign === '+') hunk.rows.push({ kind: 'add', text, new: newLine++ });
    else if (sign === '-') hunk.rows.push({ kind: 'delete', text, old: oldLine++ });
    else if (sign === ' ')
      hunk.rows.push({ kind: 'context', text, old: oldLine++, new: newLine++ });
    else if (sign === '\\') hunk.rows.push({ kind: 'note', text: line.slice(2) });
  }
  return parsed;
}

export interface SplitRow {
  left?: DiffRow;
  right?: DiffRow;
}

/** Side by side: unchanged lines on both sides; removed lines beside the lines that replaced them. */
export function splitRows(rows: DiffRow[]): SplitRow[] {
  const result: SplitRow[] = [];
  let index = 0;
  while (index < rows.length) {
    const row = rows[index]!;
    if (row.kind === 'context') {
      result.push({ left: row, right: row });
      index += 1;
      continue;
    }
    if (row.kind === 'note') {
      // "No newline at end of file" belongs to the side of the line before it.
      const before = rows[index - 1];
      result.push(before?.kind === 'add' ? { right: row } : { left: row });
      index += 1;
      continue;
    }
    const removed: DiffRow[] = [];
    const added: DiffRow[] = [];
    while (rows[index]?.kind === 'delete') removed.push(rows[index++]!);
    while (rows[index]?.kind === 'add') added.push(rows[index++]!);
    for (let pair = 0; pair < Math.max(removed.length, added.length); pair += 1) {
      const left = removed[pair];
      const right = added[pair];
      result.push({ ...(left ? { left } : {}), ...(right ? { right } : {}) });
    }
  }
  return result;
}

export interface Segment {
  text: string;
  changed: boolean;
}

/** Words, runs of spaces and single other characters (so each CJK character is its own). */
const tokens = (text: string) => text.match(/[A-Za-z0-9_]+|\s+|[^A-Za-z0-9_\s]/gu) ?? [];

/** The longest common run of tokens decides what stayed; the rest of each line changed. */
export function wordDiff(before: string, after: string): [Segment[], Segment[]] | undefined {
  const a = tokens(before);
  const b = tokens(after);
  if (a.length * b.length > 160_000) return undefined;
  const lengths = Array.from({ length: a.length + 1 }, () => new Uint16Array(b.length + 1));
  for (let i = a.length - 1; i >= 0; i -= 1)
    for (let j = b.length - 1; j >= 0; j -= 1)
      lengths[i]![j] =
        a[i] === b[j]
          ? lengths[i + 1]![j + 1]! + 1
          : Math.max(lengths[i + 1]![j]!, lengths[i]![j + 1]!);
  const left: Segment[] = [];
  const right: Segment[] = [];
  const push = (into: Segment[], text: string, changed: boolean) => {
    const last = into.at(-1);
    if (last && last.changed === changed) last.text += text;
    else into.push({ text, changed });
  };
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      push(left, a[i]!, false);
      push(right, b[j]!, false);
      i += 1;
      j += 1;
    } else if (lengths[i + 1]![j]! >= lengths[i]![j + 1]!) push(left, a[i++]!, true);
    else push(right, b[j++]!, true);
  }
  while (i < a.length) push(left, a[i++]!, true);
  while (j < b.length) push(right, b[j++]!, true);
  return [left, right];
}

/** Each removed line paired with the line that replaced it, by their changed words. */
export function changedWords(rows: DiffRow[]): Map<DiffRow, Segment[]> {
  const result = new Map<DiffRow, Segment[]>();
  let index = 0;
  while (index < rows.length) {
    if (rows[index]!.kind !== 'delete') {
      index += 1;
      continue;
    }
    const removed: DiffRow[] = [];
    const added: DiffRow[] = [];
    while (rows[index]?.kind === 'delete') removed.push(rows[index++]!);
    while (rows[index]?.kind === 'add') added.push(rows[index++]!);
    for (let pair = 0; pair < Math.min(removed.length, added.length); pair += 1) {
      const words = wordDiff(removed[pair]!.text, added[pair]!.text);
      if (!words) continue;
      result.set(removed[pair]!, words[0]);
      result.set(added[pair]!, words[1]);
    }
  }
  return result;
}
