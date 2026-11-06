import type { PluginTheme } from '@aura/contracts/generated';

/** The colours a theme may set, by the window's own names; the same list Core checks. */
const COLORS = [
  'ink',
  'muted',
  'quiet',
  'line',
  'line_strong',
  'canvas',
  'surface',
  'surface_muted',
  'surface_selected',
  'accent',
  'accent_strong',
  'accent_soft',
  'success',
  'warning',
  'danger',
  'focus',
  'success_soft',
  'warning_soft',
  'danger_soft',
] as const;
const RADII = ['sm', 'md', 'lg', 'xl'] as const;
/** Each font list ends in the generic family of its kind. */
const FONTS = [
  ['ui', 'sans-serif'],
  ['mono', 'monospace'],
  ['serif', 'serif'],
] as const;
/** Aura's own shades that no theme names, taken from the ones it does so that they follow it. */
const DERIVED = [
  '--pane: var(--surface-muted);',
  '--surface-hover: var(--surface-muted);',
  '--faint: var(--quiet);',
  '--line-soft: var(--line);',
  '--control-line: var(--line-strong);',
  '--accent-hover: var(--accent-strong);',
  '--pixel-shadow: var(--line-strong);',
  '--success-line: color-mix(in srgb, var(--success) 30%, var(--success-soft));',
  '--warning-line: color-mix(in srgb, var(--warning) 30%, var(--warning-soft));',
  '--danger-line: color-mix(in srgb, var(--danger) 30%, var(--danger-soft));',
  '--danger-shade: var(--danger-soft);',
  '--success-dot: var(--success);',
  '--warning-dot: var(--warning);',
  '--danger-dot: var(--danger);',
  '--idle-dot: var(--quiet);',
];
const HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/u;
/** Nothing that could end the quotes a name is put in, or the declaration. */
const FONT_NAME = /^[\p{L}\p{N} ._-]{1,64}$/u;
const MAX_FONTS = 8;
const MAX_RADIUS = 24;

/**
 * The style sheet of a theme Core checked, checked again here: only the window's own variables
 * and only values that cannot end a declaration. Undefined when nothing in it can be used.
 */
export function themeCss(theme: PluginTheme): string | undefined {
  const declarations: string[] = [];
  for (const key of COLORS) {
    const value = theme.colors[key];
    if (typeof value === 'string' && HEX.test(value))
      declarations.push(`--${key.replace(/_/gu, '-')}: ${value};`);
  }
  for (const [slot, generic] of FONTS) {
    const names: unknown = theme.fonts[slot];
    if (
      Array.isArray(names) &&
      names.length > 0 &&
      names.length <= MAX_FONTS &&
      names.every((name) => typeof name === 'string' && FONT_NAME.test(name))
    )
      declarations.push(
        `--font-${slot}: ${names.map((name: string) => `"${name}"`).join(', ')}, ${generic};`,
      );
  }
  for (const key of RADII) {
    const value = theme.radius[key];
    if (Number.isInteger(value) && value! >= 0 && value! <= MAX_RADIUS)
      declarations.push(`--radius-${key}: ${value}px;`);
  }
  if (declarations.length === 0) return undefined;
  const scheme = theme.scheme === 'dark' ? 'dark' : 'light';
  return [
    ':root {',
    `  color-scheme: ${scheme};`,
    '  color: var(--ink);',
    '  background: var(--canvas);',
    ...declarations.map((line) => `  ${line}`),
    ...DERIVED.map((line) => `  ${line}`),
    '}',
    '',
  ].join('\n');
}

let sheet: CSSStyleSheet | undefined;

/**
 * Lays the theme over Aura's own variables with a constructed style sheet (the page's policy
 * allows no inline styles); none takes it away again.
 */
export function applyThemeSheet(css: string | undefined): void {
  // Aura's own theme needs nothing from the document, until a theme was laid over it.
  if (css === undefined && sheet === undefined) return;
  const adopted = [...document.adoptedStyleSheets];
  if (css === undefined) {
    if (adopted.includes(sheet!))
      document.adoptedStyleSheets = adopted.filter((each) => each !== sheet);
    return;
  }
  sheet ??= new CSSStyleSheet();
  sheet.replaceSync(css);
  if (!adopted.includes(sheet)) document.adoptedStyleSheets = [...adopted, sheet];
}
