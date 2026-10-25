import type { ITheme } from '@xterm/xterm';

export type Rgb = readonly [number, number, number];
const ANSI_KEYS = [
  'black',
  'red',
  'green',
  'yellow',
  'blue',
  'magenta',
  'cyan',
  'white',
  'brightBlack',
  'brightRed',
  'brightGreen',
  'brightYellow',
  'brightBlue',
  'brightMagenta',
  'brightCyan',
  'brightWhite',
] as const;
// The approved xterm 6.0 palette. Slots 16..255 use the standard cube and gray ramp.
const DEFAULT_ANSI = [
  '#2e3436',
  '#cc0000',
  '#4e9a06',
  '#c4a000',
  '#3465a4',
  '#75507b',
  '#06989a',
  '#d3d7cf',
  '#555753',
  '#ef2929',
  '#8ae234',
  '#fce94f',
  '#729fcf',
  '#ad7fa8',
  '#34e2e2',
  '#eeeeec',
];
const LIGHT_ANSI: Pick<ITheme, (typeof ANSI_KEYS)[number]> = {
  black: '#242724',
  red: '#b42318',
  green: '#2f6b39',
  yellow: '#876100',
  blue: '#2558a3',
  magenta: '#85418d',
  cyan: '#1c6c75',
  white: '#636963',
  brightBlack: '#525952',
  brightRed: '#cf352c',
  brightGreen: '#3b7b42',
  brightYellow: '#967000',
  brightBlue: '#346abe',
  brightMagenta: '#9a4ba3',
  brightCyan: '#267b86',
  brightWhite: '#737973',
};

function rgb(color: string | undefined): Rgb | undefined {
  if (!color) return undefined;
  if (/^#[\da-f]{6}$/i.test(color)) {
    return [
      Number.parseInt(color.slice(1, 3), 16),
      Number.parseInt(color.slice(3, 5), 16),
      Number.parseInt(color.slice(5, 7), 16),
    ];
  }
  if (/^#[\da-f]{3}$/i.test(color)) {
    return [
      Number.parseInt(color[1]!.repeat(2), 16),
      Number.parseInt(color[2]!.repeat(2), 16),
      Number.parseInt(color[3]!.repeat(2), 16),
    ];
  }
  const parts = /^rgba?\(\s*(\d+)\s*[, ]\s*(\d+)\s*[, ]\s*(\d+)(?:\s*[,/]\s*[\d.]+)?\s*\)$/i.exec(
    color,
  );
  if (parts) return [Number(parts[1]), Number(parts[2]), Number(parts[3])];
  return undefined;
}
function luminance(value: Rgb) {
  const [r, g, b] = value.map((part) => {
    const channel = part / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return r! * 0.2126 + g! * 0.7152 + b! * 0.0722;
}

/** Use the app's semantic colors, with readable ANSI hues only on a light canvas. */
export function terminalTheme(base: ITheme): ITheme {
  const background = rgb(base.background);
  return background && luminance(background) > 0.5 ? { ...base, ...LIGHT_ANSI } : { ...base };
}

/** Match against the palette actually configured on this terminal, including its ANSI overrides. */
export function terminalPalette(theme: ITheme): readonly Rgb[] {
  const colors: Rgb[] = ANSI_KEYS.map((key, index) => rgb(theme[key]) ?? rgb(DEFAULT_ANSI[index])!);
  const levels = [0, 95, 135, 175, 215, 255];
  for (const r of levels) for (const g of levels) for (const b of levels) colors.push([r, g, b]);
  for (let gray = 8; gray <= 238; gray += 10) colors.push([gray, gray, gray]);
  return colors;
}

export function nearestPaletteColor(
  palette: readonly Rgb[],
  r: number,
  g: number,
  b: number,
): number {
  let nearest = 0;
  let distance = Infinity;
  for (let index = 0; index < palette.length; index++) {
    const color = palette[index]!;
    const candidate = (color[0] - r) ** 2 + (color[1] - g) ** 2 + (color[2] - b) ** 2;
    if (candidate < distance) {
      distance = candidate;
      nearest = index;
    }
  }
  return nearest;
}
