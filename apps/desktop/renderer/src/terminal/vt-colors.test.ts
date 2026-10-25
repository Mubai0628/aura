import { Terminal } from '@xterm/xterm';
import { expect, it } from 'vitest';
import { nearestPaletteColor, terminalPalette, terminalTheme, type Rgb } from './terminal-colors';
import { VtColorConverter } from './vt-colors';

const encode = (text: string) => new TextEncoder().encode(text);
const decode = (data: Uint8Array) => new TextDecoder().decode(data);
const palette = terminalPalette(terminalTheme({ background: '#fbfbfa' }));
function convert(text: string, stride = 1) {
  const converter = new VtColorConverter(palette);
  const bytes = encode(text);
  const chunks: number[] = [];
  for (let index = 0; index < bytes.length; index += stride) {
    chunks.push(...converter.write(bytes.slice(index, index + stride)));
  }
  return decode(Uint8Array.from(chunks));
}

it('matches RGB to all 256 actual colors, with deterministic nearest and tie choices', () => {
  expect(palette).toHaveLength(256);
  for (const [r, g, b] of [
    [170, 20, 100],
    [10, 140, 170],
    [120, 123, 119],
    [255, 255, 255],
  ] as Rgb[]) {
    const distances = palette.map(
      (color) => (color[0] - r) ** 2 + (color[1] - g) ** 2 + (color[2] - b) ** 2,
    );
    expect(nearestPaletteColor(palette, r, g, b)).toBe(distances.indexOf(Math.min(...distances)));
  }
  const red = palette[1]!;
  expect(nearestPaletteColor(palette, ...red)).toBe(1);
  expect(
    nearestPaletteColor(
      [
        [0, 0, 0],
        [2, 0, 0],
      ],
      1,
      0,
      0,
    ),
  ).toBe(0);
});
it('uses the computed canvas for light and dark themes in hex and CSS rgb forms', () => {
  for (const background of ['#fff', '#fbfbfa', 'rgb(251, 251, 250)', 'rgb(251 251 250)']) {
    expect(terminalTheme({ background }).green).toBe('#2f6b39');
  }
  for (const background of ['#111', '#181b19', 'rgb(24, 27, 25)', 'rgb(24 27 25)']) {
    expect(terminalTheme({ background })).toEqual({ background });
  }
});
it('handles colon, mixed colon/semicolon and indexed colors without interpreting color channels as SGR', () => {
  const input = '\x1b[4:3;38:2::175:0:95;48;2:0:0:135:175mA\x1b[38;5;38;48:5:31mB\x1b[0m';
  expect(convert(input)).toBe('\x1b[4:3;38;5;125;48;5;31mA\x1b[38;5;38;48:5:31mB\x1b[0m');
});
it('normalizes default and masked RGB components exactly as the xterm SGR parser does', () => {
  const missing = nearestPaletteColor(palette, 175, 0, 0);
  expect(convert('\x1b[38;2;175m')).toBe(`\x1b[38;5;${missing}m`);
  expect(convert('\x1b[48;2;256;391;431m')).toBe('\x1b[48;5;31m');
  expect(convert('\x1b[38:2::175:0:95m')).toBe('\x1b[38;5;125m');
});
it('preserves interrupted, cancelled and non-SGR CSI while converting the following complete color', () => {
  const interrupted = '\x1b[38;2;175;\x1b[31m';
  const cancelled = '\x1b[48;2;0;\x18remaining';
  const controls = '\x1b[2J\x1b[?25l\x1b[1;1H\x1b[?25h';
  expect(convert(`${interrupted}${cancelled}${controls}\x1b[38;2;175;0;95mX`)).toBe(
    `${interrupted}${cancelled}${controls}\x1b[38;5;125mX`,
  );
  expect(convert('\x1b[38;2;175;\x07\x00\x7f0;95mX')).toBe('\x07\x00\x7f\x1b[38;5;125mX');
});
it('handles UTF-8 C1 controls without confusing multibyte text continuation bytes with CSI', () => {
  const text = 'Û £ 中文🙂'; // Û contains 9B as a UTF-8 continuation byte, not a C1 CSI.
  expect(convert(`${text}\u009b38;2;175;0;95mX\u009b0m`)).toBe(`${text}\x1b[38;5;125mX\u009b0m`);
  const string = '\u009d0;title [38;2;175;0;95m\u009c';
  expect(convert(`${string}\x1b[38;2;175;0;95mX`)).toBe(`${string}\x1b[38;5;125mX`);
  const interrupted = `\x1b${string}`;
  expect(convert(`${interrupted}\x1b[38;2;175;0;95mX`)).toBe(`${interrupted}\x1b[38;5;125mX`);
});
it('ends OSC or DCS at an ESC before a color CSI, as xterm does', () => {
  for (const start of ['\x1b]title', '\x1bPpayload', '\x1b_hidden', '\u009dtitle']) {
    expect(convert(`${start}\x1b[38;2;175;0;95mX`)).toBe(`${start}\x1b[38;5;125mX`);
  }
});
it('bounds an unfinished excessive CSI and resumes after cancellation or its final byte', () => {
  const prefix = `\x1b[38;2;${'0'.repeat(5000)}`;
  expect(convert(`${prefix};0;0mAFTER\x1b[38;2;175;0;95mX`, 100)).toBe(
    '\x1b[\x18AFTER\x1b[38;5;125mX',
  );
  expect(convert(`${prefix}\x18AFTER`, 100)).toBe('\x1b[\x18\x18AFTER');
});
it('does not enable previously ignored parameters when shortening a long RGB SGR', () => {
  const prefix = Array.from({ length: 27 }, () => '1').join(';');
  expect(convert(`\x1b[${prefix};38;2;175;0;95;48;2;0;135;175mX`)).toBe(
    `\x1b[${prefix};38;5;125mX`,
  );
});
it('does not revive RGB subparameters previously ignored at the xterm subparameter limit', () => {
  const prefix = `4:${Array.from({ length: 29 }, () => '1').join(':')}`;
  const foreground = nearestPaletteColor(palette, 175, 0, 0);
  // Only three of the first RGB's subparameters fit. The trailing RGB's subparameters are
  // entirely ignored by xterm; shortening the first RGB must not make them active again.
  expect(convert(`\x1b[${prefix};38:2::175:0:95;48:2::0:135:175mX`)).toBe(
    `\x1b[${prefix};38;5;${foreground};48mX`,
  );
});
it('leaves xterm cells indexed for UTF-8, foreground, background, flags and reset', async () => {
  const terminal = new Terminal({ cols: 30, rows: 4, allowProposedApi: true });
  try {
    const converter = new VtColorConverter(palette);
    const input = encode('\x1b[1;38;2;175;0;95;48;2;0;135;175m字A\x1b[0mB');
    for (const byte of input) {
      await new Promise<void>((resolve) =>
        terminal.write(converter.write(Uint8Array.of(byte)), resolve),
      );
    }
    const line = terminal.buffer.active.getLine(0)!;
    expect(line.translateToString(true)).toBe('字AB');
    const colored = line.getCell(2)!;
    expect(colored.isFgPalette()).toBeTruthy();
    expect(colored.isFgRGB()).toBeFalsy();
    expect(colored.getFgColor()).toBe(125);
    expect(colored.isBgPalette()).toBeTruthy();
    expect(colored.isBgRGB()).toBeFalsy();
    expect(colored.getBgColor()).toBe(31);
    expect(colored.isBold()).toBeTruthy();
    expect(line.getCell(3)!.isFgDefault()).toBeTruthy();
    expect(line.getCell(3)!.isBgDefault()).toBeTruthy();
    expect(line.getCell(3)!.isBold()).toBeFalsy();
  } finally {
    terminal.dispose();
  }
});
it('keeps RGB black approximations stable through real xterm bold and subsequent SGR 22', async () => {
  for (const [background, channels, expected] of [
    ['#fbfbfa', '36;39;36', 235], // The light theme's black; its gray-ramp neighbor is 38,38,38.
    ['#181b19', '46;52;54', 236], // xterm's default black; nearest fixed gray is 48,48,48.
  ] as const) {
    const theme = terminalTheme({ background });
    const terminal = new Terminal({ cols: 20, rows: 2, theme, allowProposedApi: true });
    try {
      const converter = new VtColorConverter(terminalPalette(theme));
      await new Promise<void>((resolve) =>
        terminal.write(
          converter.write(
            encode(
              `\x1b[38;2;${channels};48;2;${channels}mN\x1b[1mB\x1b[22mN\x1b[0;31mA\x1b[1mB\x1b[22mA`,
            ),
          ),
          resolve,
        ),
      );
      const line = terminal.buffer.active.getLine(0)!;
      expect(line.translateToString(true)).toBe('NBNABA');
      expect([0, 1, 2].map((index) => !!line.getCell(index)!.isBold())).toEqual([
        false,
        true,
        false,
      ]);
      expect([0, 1, 2].map((index) => line.getCell(index)!.getFgColor())).toEqual([
        expected,
        expected,
        expected,
      ]);
      expect([0, 1, 2].map((index) => line.getCell(index)!.getBgColor())).toEqual([
        expected,
        expected,
        expected,
      ]);
      // Ordinary ANSI retains xterm's existing bold/bright behavior; only RGB uses fixed slots.
      expect(terminal.options.drawBoldTextInBrightColors).toBe(true);
      expect([3, 4, 5].map((index) => line.getCell(index)!.getFgColor())).toEqual([1, 1, 1]);
      expect([3, 4, 5].map((index) => !!line.getCell(index)!.isBold())).toEqual([
        false,
        true,
        false,
      ]);
    } finally {
      terminal.dispose();
    }
  }
});
