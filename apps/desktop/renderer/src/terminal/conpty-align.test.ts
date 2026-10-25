import { Terminal } from '@xterm/xterm';
import { afterEach, expect, it } from 'vitest';
import { applyRepaint, repaintComplete, splitRepaint } from './conpty-align';
import { CONPTY } from './conpty-resize';

const encode = (text: string) => new TextEncoder().encode(text);
const write = (terminal: Terminal, data: string | Uint8Array) =>
  new Promise<void>((resolve) => {
    terminal.write(data, resolve);
  });

// The inbox ConPTY as captured on Windows 11 26200: it keeps one screen. After a resize
// it reflows that screen on its own, drops from the top whatever no longer fits, and repaints from
// the home position: each line as one run that wraps by itself, ESC [ K and CR LF between lines,
// the rows below cleared, then the cursor. The cursor is at the end of the last line.
class InboxConpty {
  lines = [''];
  constructor(
    private cols: number,
    private rows: number,
  ) {}
  private height(line: string, cursor: boolean) {
    return cursor
      ? Math.floor(line.length / this.cols) + 1
      : Math.max(1, Math.ceil(line.length / this.cols));
  }
  private used() {
    return this.lines.reduce(
      (sum, line, index) => sum + this.height(line, index === this.lines.length - 1),
      0,
    );
  }
  private fit() {
    for (let used = this.used(); used > this.rows; used--) {
      const first = this.lines[0] ?? '';
      if (this.lines.length > 1 && this.height(first, false) === 1) this.lines.shift();
      else this.lines[0] = first.slice(this.cols);
    }
  }
  print(text: string) {
    const [head = '', ...rest] = text.split('\r\n');
    this.lines.push(`${this.lines.pop() ?? ''}${head}`, ...rest);
    this.fit();
  }
  resize(cols: number, rows: number) {
    this.cols = cols;
    this.rows = rows;
    this.fit();
  }
  repaint() {
    const used = this.used();
    const last = this.lines[this.lines.length - 1] ?? '';
    const cursorY = used - 1;
    const cursorX = last.length % this.cols;
    return `\x1b[?25l\x1b[H${this.lines.join('\x1b[K\r\n')}\x1b[K${'\r\n\x1b[K'.repeat(this.rows - used)}\x1b[${cursorY + 1};${cursorX + 1}H\x1b[?25h`;
  }
}

const terminals: Terminal[] = [];
afterEach(() => {
  for (const terminal of terminals.splice(0)) terminal.dispose();
});

function session(cols: number, rows: number) {
  const terminal = new Terminal({
    cols,
    rows,
    scrollback: 1000,
    windowsPty: CONPTY,
    reflowCursorLine: true,
  });
  terminals.push(terminal);
  const conpty = new InboxConpty(cols, rows);
  return {
    terminal,
    output: async (text: string) => {
      conpty.print(text);
      await write(terminal, text);
    },
    resize: async (nextCols: number, nextRows: number, align = true) => {
      terminal.resize(nextCols, nextRows);
      conpty.resize(nextCols, nextRows);
      const repaint = encode(conpty.repaint());
      await (align ? applyRepaint(terminal, repaint) : write(terminal, repaint));
    },
  };
}

// The whole buffer as logical lines.
function history(terminal: Terminal) {
  const buffer = terminal.buffer.active;
  const lines: string[] = [];
  for (let y = 0; y < buffer.length; y++) {
    const line = buffer.getLine(y);
    if (!line) continue;
    if (line.isWrapped && lines.length) lines[lines.length - 1] += line.translateToString(false);
    else lines.push(line.translateToString(false));
  }
  const trimmed = lines.map((line) => line.trimEnd());
  while (trimmed.length && !trimmed[trimmed.length - 1]) trimmed.pop();
  return trimmed;
}

const PROMPT = 'PS C:\\Users\\owner\\AppData\\Local\\Temp\\aura-l02-snap-T81jLi\\workspace> ';
const LINES = Array.from(
  { length: 70 },
  (_, index) => `N${String(index + 1).padStart(3, '0')} ${'0123456789'.repeat(7)} END`,
);
const EXPECTED = [`${PROMPT}list`, ...LINES, PROMPT.trimEnd()];

async function listed(cols = 103, rows = 41) {
  const s = session(cols, rows);
  await s.output(`${PROMPT}list\r\n${LINES.join('\r\n')}\r\n${PROMPT}`);
  return s;
}

it('loses the lines above ConPTY’s screen when the repaint is written as it comes', async () => {
  // Narrowed to the panel minimum, widened again.
  const s = await listed();
  await s.resize(33, 40, false);
  await s.resize(103, 41, false);
  const lost = EXPECTED.filter((line) => !history(s.terminal).includes(line));
  expect(lost.length).toBeGreaterThan(20);
});

it('keeps every line once and whole when the panel is narrowed and widened again', async () => {
  const s = await listed();
  await s.resize(33, 40);
  expect(history(s.terminal)).toEqual(EXPECTED);
  await s.resize(103, 41);
  expect(history(s.terminal)).toEqual(EXPECTED);
  // ConPTY's screen sits at the top after widening, as ConPTY has it.
  expect(s.terminal.buffer.active.cursorY).toBeLessThan(20);
});

it('keeps every line through repeated resizes in both directions', async () => {
  const s = await listed();
  for (const [cols, rows] of [
    [33, 40],
    [103, 41],
    [60, 41],
    [33, 38],
    [120, 41],
    [45, 41],
    [103, 41],
  ] as const) {
    await s.resize(cols, rows);
    expect(history(s.terminal), `${cols}x${rows}`).toEqual(EXPECTED);
  }
  // New output still lines up with ConPTY after all of that.
  await s.output('next\r\nN071 done\r\n');
  expect(history(s.terminal).slice(-3)).toEqual([LINES[69], `${PROMPT}next`, 'N071 done']);
});

it('keeps the history when narrowing a short screen with blank rows below the prompt', async () => {
  const s = await listed();
  // After widening, ConPTY's few rows sit at the top with blank rows below; narrowing then lets
  // ConPTY use those rows while xterm would have scrolled.
  await s.resize(33, 40);
  await s.resize(103, 41);
  await s.resize(50, 41);
  expect(history(s.terminal)).toEqual(EXPECTED);
});

it('writes back a cut-off top line with its colors', async () => {
  const s = session(103, 41);
  const colored = LINES.map((line, index) => (index === 57 ? `\x1b[34m${line}\x1b[0m` : line));
  await s.output(`${PROMPT}list\r\n${colored.join('\r\n')}\r\n${PROMPT}`);
  await s.resize(33, 40);
  await s.resize(103, 41);
  expect(history(s.terminal)).toEqual(EXPECTED);
  const buffer = s.terminal.buffer.active;
  const row = [...Array(buffer.length).keys()].find((y) =>
    buffer.getLine(y)?.translateToString(true).startsWith('N058'),
  );
  const cell = buffer.getLine(row ?? -1)?.getCell(0);
  expect(cell?.isFgPalette() && cell.getFgColor()).toBe(4);
});

it('writes output without a repaint, and the alternate screen, as it comes', async () => {
  const s = await listed();
  await applyRepaint(s.terminal, encode('more\r\n'));
  expect(history(s.terminal).slice(-1)).toEqual([`${PROMPT}more`]);
  await write(s.terminal, '\x1b[?1049h');
  expect(s.terminal.buffer.active.type).toBe('alternate');
  await applyRepaint(s.terminal, encode('\x1b[Hfull screen'));
  expect(s.terminal.buffer.active.getLine(0)?.translateToString(true)).toBe('full screen');
});

it('finds the repaint between other output', () => {
  const data = encode('before\x1b[?25l\x1b[Hscreen\x1b[K\x1b[3;5H\x1b[?25hafter');
  const parts = splitRepaint(data);
  const text = (bytes?: Uint8Array) => new TextDecoder().decode(bytes);
  expect(text(parts?.before)).toBe('before');
  expect(text(parts?.repaint)).toBe('\x1b[?25l\x1b[Hscreen\x1b[K\x1b[3;5H\x1b[?25h');
  expect(text(parts?.after)).toBe('after');
  expect(repaintComplete(data)).toBe(true);
  expect(repaintComplete(encode('\x1b[?25l\x1b[Hscreen'))).toBe(false);
  expect(splitRepaint(encode('plain output'))).toBeUndefined();
});
