import type { ITerminalOptions } from '@xterm/xterm';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { TerminalRecord } from './store';
import { disposeTerminalEngine, terminalEngine } from './xterm-runtime';

const state = vi.hoisted(() => ({
  options: [] as ITerminalOptions[],
  writes: [] as Uint8Array[],
  consumers: new Map<string, (data: Uint8Array) => void>(),
}));
vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    constructor(options: ITerminalOptions) {
      state.options.push(options);
    }
    loadAddon() {}
    open() {}
    onData() {
      return { dispose() {} };
    }
    attachCustomKeyEventHandler() {}
    write(data: Uint8Array, done?: () => void) {
      state.writes.push(data);
      done?.();
    }
    dispose() {}
  },
}));
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class {} }));
vi.mock('./store', () => ({
  attachTerminalConsumer(record: TerminalRecord, consume: (data: Uint8Array) => void) {
    state.consumers.set(record.view.terminal_id, consume);
  },
  configureTerminalRenderer() {},
  terminalApi: vi.fn(),
  terminalFailure: vi.fn(),
}));
vi.mock('./csp-styles', () => ({ adoptTerminalStyles: () => () => {} }));

const encode = (text: string) => new TextEncoder().encode(text);
const decode = (bytes: Uint8Array) => new TextDecoder().decode(bytes);
const written = () => decode(Uint8Array.from(state.writes.flatMap((bytes) => [...bytes])));
let sequence = 0;
const ids: string[] = [];
function fixture() {
  const id = `color-test-${sequence++}`;
  ids.push(id);
  const record = { view: { terminal_id: id } } as unknown as TerminalRecord;
  const engine = terminalEngine(record);
  const feed = (bytes: Uint8Array) => state.consumers.get(id)!(bytes);
  return { engine, feed };
}
beforeEach(() => {
  state.options.length = 0;
  state.writes.length = 0;
  state.consumers.clear();
  document.documentElement.style.setProperty('--canvas', '#fbfbfa');
  document.documentElement.style.setProperty('--ink', '#242724');
  vi.stubGlobal('requestAnimationFrame', () => 1);
  vi.stubGlobal('cancelAnimationFrame', () => {});
});
afterEach(() => {
  for (const id of ids.splice(0)) disposeTerminalEngine(id);
  document.documentElement.removeAttribute('style');
  vi.unstubAllGlobals();
});

// xterm 6.0's actual defaults are the fallback when no ANSI theme color is set.
const chromaticDefaults = {
  red: '#cc0000',
  green: '#4e9a06',
  yellow: '#c4a000',
  blue: '#3465a4',
  magenta: '#75507b',
  cyan: '#06989a',
  brightRed: '#ef2929',
  brightGreen: '#8ae234',
  brightYellow: '#fce94f',
  brightBlue: '#729fcf',
  brightMagenta: '#ad7fa8',
  brightCyan: '#34e2e2',
};
function luminance(hex: string) {
  const values = [1, 3, 5].map((offset) => {
    const value = Number.parseInt(hex.slice(offset, offset + 2), 16) / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return values[0]! * 0.2126 + values[1]! * 0.7152 + values[2]! * 0.0722;
}
it('gives all twelve light-background ANSI hues at least 3:1 contrast', () => {
  fixture();
  const theme = state.options[0]!.theme!;
  const contrast = Object.entries(chromaticDefaults).map(([key, fallback]) => {
    const foreground = luminance(theme[key as keyof typeof chromaticDefaults] ?? fallback);
    const background = luminance('#fbfbfa');
    return [
      key,
      (Math.max(foreground, background) + 0.05) / (Math.min(foreground, background) + 0.05),
    ];
  });
  expect(contrast.filter(([, ratio]) => Number(ratio) < 3)).toEqual([]);
});
it('leaves the dark-background ANSI palette at xterm defaults', () => {
  document.documentElement.style.setProperty('--canvas', '#181b19');
  fixture();
  for (const key of Object.keys(chromaticDefaults)) {
    expect(state.options[0]!.theme).not.toHaveProperty(key);
  }
});
it('converts truecolor foreground and background at every output split before xterm writes', async () => {
  const input = encode('中文\x1b[1;38;2;175;0;95;48;2;0;135;175mRGB\x1b[0m\r\n');
  for (let split = 0; split <= input.length; split++) {
    const { engine, feed } = fixture();
    state.writes.length = 0;
    feed(input.slice(0, split));
    await engine.drain();
    feed(input.slice(split));
    await engine.drain();
    expect(written()).toBe('中文\x1b[1;38;5;125;48;5;31mRGB\x1b[0m\r\n');
  }
});
it('converts ConPTY repaint bytes through the same ingress while retaining cursor markers', async () => {
  const { engine, feed } = fixture();
  const hold = engine.hold();
  const output = hold.repaint();
  feed(encode('\x1b[?25l\x1b[H\x1b[38;2;175;'));
  feed(encode('0;95mREPAINT\x1b[K\x1b[2;1H\x1b[?25h'));
  expect(decode(await output)).toBe('\x1b[?25l\x1b[H\x1b[38;5;125mREPAINT\x1b[K\x1b[2;1H\x1b[?25h');
  hold.release();
});
it('keeps opaque OSC/DCS contents and all other VT commands unchanged', async () => {
  const { engine, feed } = fixture();
  const opaque = '\x1b]0;title [38;2;1;2;3m\x07\x1bPpayload [48;2;4;5;6m\x1b\\';
  const text = `${opaque}\x1b[2J\x1b[3;4H\x1b[38;5;31m保留\x1b[48:2::0:135:175mRGB\x1b[0m`;
  for (const byte of encode(text)) {
    feed(Uint8Array.of(byte));
  }
  await engine.drain();
  expect(written()).toBe(`${opaque}\x1b[2J\x1b[3;4H\x1b[38;5;31m保留\x1b[48;5;31mRGB\x1b[0m`);
});
