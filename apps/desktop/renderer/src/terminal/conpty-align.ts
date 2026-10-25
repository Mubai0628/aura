import { Terminal, type IBufferCell, type IBufferLine } from '@xterm/xterm';
import { CONPTY } from './conpty-resize';

// After a resize the inbox ConPTY repaints the one screen it keeps: it reflows that screen on its
// own, paints it from the top row down and clears the rows below. xterm reflows its whole buffer
// instead and keeps the cursor on the same row, so the repaint landed on rows that still showed
// history and those lines were lost: about 25 lines after a narrowed panel was widened again. Before the repaint is written, xterm's rows are moved so that its cursor sits on
// ConPTY's cursor row. The repaint then covers the same lines, and everything above it stays in
// scrollback.

const HOME = [0x1b, 0x5b, 0x48]; // ESC [ H: every repaint starts by homing the cursor
const HIDE = [0x1b, 0x5b, 0x3f, 0x32, 0x35, 0x6c]; // ESC [ ? 2 5 l, just before it
const SHOW = [0x1b, 0x5b, 0x3f, 0x32, 0x35, 0x68]; // ESC [ ? 2 5 h, the end of the repaint
// ConPTY's top line can be a cut-off piece of a longer line; it spans a few rows at most.
const RESTORE_ROWS = 8;

function find(data: Uint8Array, sequence: readonly number[], from = 0) {
  search: for (let index = from; index <= data.length - sequence.length; index++) {
    for (let offset = 0; offset < sequence.length; offset++) {
      if (data[index + offset] !== sequence[offset]) continue search;
    }
    return index;
  }
  return -1;
}

export function repaintComplete(data: Uint8Array) {
  const home = find(data, HOME);
  return home >= 0 && find(data, SHOW, home) >= 0;
}

export function splitRepaint(data: Uint8Array) {
  let start = find(data, HOME);
  if (start < 0) return undefined;
  if (start >= HIDE.length && find(data, HIDE, start - HIDE.length) === start - HIDE.length) {
    start -= HIDE.length;
  }
  const show = find(data, SHOW, start);
  const end = show < 0 ? data.length : show + SHOW.length;
  return {
    before: data.subarray(0, start),
    repaint: data.subarray(start, end),
    after: data.subarray(end),
  };
}

const write = (terminal: Terminal, data: Uint8Array | string) =>
  new Promise<void>((resolve) => {
    terminal.write(data, resolve);
  });

export interface ConptyScreen {
  cursorY: number;
  rows: string[];
}

// What ConPTY shows after the repaint, read by playing the repaint on a blank terminal.
export async function readScreen(repaint: Uint8Array, cols: number, rows: number) {
  const scratch = new Terminal({ cols, rows, scrollback: rows, windowsPty: CONPTY });
  try {
    await write(scratch, repaint);
    const buffer = scratch.buffer.active;
    // A repaint paints exactly one screen. Anything that scrolled was other output.
    if (buffer.baseY > 0) return undefined;
    const screen: ConptyScreen = { cursorY: buffer.cursorY, rows: [] };
    for (let y = 0; y < rows; y++)
      screen.rows.push(buffer.getLine(y)?.translateToString(true) ?? '');
    return screen;
  } finally {
    scratch.dispose();
  }
}

// Moves xterm's rows so that the cursor is on `target`: rows above go into scrollback, or come
// back from it. Rows below the cursor are blank here and are repainted by ConPTY anyway.
export function placeCursorRow(terminal: Terminal, target: number) {
  const buffer = terminal.buffer.active;
  const { cols, rows } = terminal;
  const y = buffer.cursorY;
  if (buffer.type !== 'normal' || target === y || target < 0 || target >= rows) return;
  terminal.resize(cols, y + 1);
  if (target > y) {
    // With the conpty option xterm adds blank rows when it grows; without it, it takes them back
    // from scrollback, which is what ConPTY shows above its cursor.
    terminal.options.windowsPty = {};
    terminal.resize(cols, target + 1);
    terminal.options.windowsPty = CONPTY;
  } else {
    terminal.resize(cols, target + 1);
  }
  terminal.resize(cols, rows);
}

interface SavedRow {
  text: string;
  vt: string;
}

function color(palette: boolean, rgb: boolean, value: number, base: number) {
  if (palette && value < 8) return [`${base + value}`];
  if (palette && value < 16) return [`${base + 60 + value - 8}`];
  if (palette) return [`${base + 8}`, '5', `${value}`];
  if (rgb)
    return [
      `${base + 8}`,
      '2',
      `${(value >> 16) & 255}`,
      `${(value >> 8) & 255}`,
      `${value & 255}`,
    ];
  return [];
}

function style(cell: IBufferCell) {
  const codes = ['0'];
  const flags: [number, string][] = [
    [cell.isBold(), '1'],
    [cell.isDim(), '2'],
    [cell.isItalic(), '3'],
    [cell.isUnderline(), '4'],
    [cell.isBlink(), '5'],
    [cell.isInverse(), '7'],
    [cell.isInvisible(), '8'],
    [cell.isStrikethrough(), '9'],
    [cell.isOverline(), '53'],
  ];
  for (const [on, code] of flags) if (on) codes.push(code);
  codes.push(...color(cell.isFgPalette(), cell.isFgRGB(), cell.getFgColor(), 30));
  codes.push(...color(cell.isBgPalette(), cell.isBgRGB(), cell.getBgColor(), 40));
  return `\x1b[${codes.join(';')}m`;
}

// The row as text with its colors, for writing it back.
function rowVt(line: IBufferLine, cols: number) {
  const cell = line.getCell(0);
  if (!cell) return '';
  let end = cols;
  while (end > 0 && line.getCell(end - 1, cell)?.getCode() === 0) end--;
  let vt = '';
  let current = '';
  for (let x = 0; x < end; x++) {
    line.getCell(x, cell);
    if (cell.getWidth() === 0) continue;
    const next = style(cell);
    if (next !== current) vt += next;
    current = next;
    vt += cell.getChars() || ' ';
  }
  return vt;
}

export function saveTopRows(terminal: Terminal, count = RESTORE_ROWS): SavedRow[] {
  const buffer = terminal.buffer.active;
  const saved: SavedRow[] = [];
  for (let y = 0; y < Math.min(count, terminal.rows); y++) {
    const line = buffer.getLine(buffer.baseY + y);
    if (!line) break;
    saved.push({ text: line.translateToString(true), vt: rowVt(line, terminal.cols) });
  }
  return saved;
}

// ConPTY's top line can be the tail of a line whose start no longer fit on its narrow screen, or
// a longer piece of it than xterm's row holds. xterm still has the whole line, so where ConPTY's
// top rows are a piece of the same line, xterm's rows are written back over them.
export function restoreCutRows(saved: SavedRow[], screen: ConptyScreen) {
  const same = saved.findIndex((row, y) => row.text === screen.rows[y]);
  if (same <= 0) return '';
  const ours = saved
    .slice(0, same)
    .map((row) => row.text)
    .join('');
  const theirs = screen.rows.slice(0, same).join('');
  if (!ours || !theirs || !(ours.endsWith(theirs) || theirs.endsWith(ours))) return '';
  const rows = saved.slice(0, same).map((row, y) => `\x1b[${y + 1};1H${row.vt}\x1b[0m\x1b[K`);
  return `\x1b7${rows.join('')}\x1b8`;
}

// Writes the output that followed a resize, with ConPTY's repaint aligned to xterm's rows.
export async function applyRepaint(terminal: Terminal, output: Uint8Array) {
  const parts = terminal.buffer.active.type === 'normal' ? splitRepaint(output) : undefined;
  if (!parts) return write(terminal, output);
  if (parts.before.length) await write(terminal, parts.before);
  const screen = await readScreen(parts.repaint, terminal.cols, terminal.rows);
  let restore = '';
  if (screen) {
    placeCursorRow(terminal, screen.cursorY);
    const saved = saveTopRows(terminal);
    await write(terminal, parts.repaint);
    restore = restoreCutRows(saved, screen);
  } else {
    await write(terminal, parts.repaint);
  }
  if (restore) await write(terminal, restore);
  if (parts.after.length) await write(terminal, parts.after);
}
