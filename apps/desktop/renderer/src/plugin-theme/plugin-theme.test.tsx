import { act, cleanup, renderHook } from '@testing-library/react';
import type { PluginTheme } from '@aura/contracts/generated';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { applyThemeSheet, themeCss } from './theme-css';
import { usePluginTheme } from './usePluginTheme';

// A plugin's theme sets the window's own colour, font and corner variables and nothing else,
// through one constructed style sheet (the page allows no inline styles). Core checked the theme;
// the window checks every value again before it becomes part of a style sheet.

// jsdom has no constructed style sheets; this stands in for the browser's.
class FakeSheet {
  text = '';
  replaceSync(text: string) {
    this.text = text;
  }
}
const adopted = () =>
  (document as unknown as { adoptedStyleSheets: FakeSheet[] }).adoptedStyleSheets;
const shown = () =>
  adopted()
    .map((sheet) => sheet.text)
    .join('');

const night = (extra: Partial<PluginTheme> = {}): PluginTheme => ({
  title: '夜间',
  scheme: 'dark',
  colors: { canvas: '#1e1f1c', line_strong: '#333' },
  fonts: { ui: ['Inter', '思源黑体'], mono: ['JetBrains Mono'] },
  radius: { md: 4 },
  ...extra,
});

let read: ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.stubGlobal('CSSStyleSheet', FakeSheet);
  Object.defineProperty(document, 'adoptedStyleSheets', {
    value: [],
    writable: true,
    configurable: true,
  });
  read = vi.fn();
  vi.stubGlobal('aura', { v1: { getPluginTheme: read } });
});

afterEach(() => {
  cleanup();
  applyThemeSheet(undefined);
  vi.unstubAllGlobals();
  delete (document as { adoptedStyleSheets?: unknown }).adoptedStyleSheets;
});

test("a theme sets only the window's own variables, in their own names", () => {
  expect(themeCss(night())).toBe(
    [
      ':root {',
      '  color-scheme: dark;',
      '  color: var(--ink);',
      '  background: var(--canvas);',
      '  --line-strong: #333;',
      '  --canvas: #1e1f1c;',
      '  --font-ui: "Inter", "思源黑体", sans-serif;',
      '  --font-mono: "JetBrains Mono", monospace;',
      '  --radius-md: 4px;',
      '  --pane: var(--surface-muted);',
      '  --surface-hover: var(--surface-muted);',
      '  --faint: var(--quiet);',
      '  --line-soft: var(--line);',
      '  --control-line: var(--line-strong);',
      '  --accent-hover: var(--accent-strong);',
      '  --pixel-shadow: var(--line-strong);',
      '  --success-line: color-mix(in srgb, var(--success) 30%, var(--success-soft));',
      '  --warning-line: color-mix(in srgb, var(--warning) 30%, var(--warning-soft));',
      '  --danger-line: color-mix(in srgb, var(--danger) 30%, var(--danger-soft));',
      '  --danger-shade: var(--danger-soft);',
      '  --success-dot: var(--success);',
      '  --warning-dot: var(--warning);',
      '  --danger-dot: var(--danger);',
      '  --idle-dot: var(--quiet);',
      '}',
      '',
    ].join('\n'),
  );
});

test('a value Core would never send does not reach the style sheet', () => {
  const css = themeCss(
    night({
      scheme: 'light',
      colors: { canvas: 'red;}body{display:none', paper: '#fff', ink: '#000', danger: '#ABC' },
      fonts: { ui: ['x";}body{', 'Inter'], serif: Array.from({ length: 9 }, () => 'Serif') },
      radius: { sm: 25, md: 2.5, huge: 4, lg: 8, xl: -1 },
    }),
  );
  expect(css).toContain('color-scheme: light;');
  expect(css).toContain('--ink: #000;');
  expect(css).toContain('--radius-lg: 8px;');
  for (const absent of [
    'display',
    'paper',
    '--canvas:',
    '--danger:',
    '--font-',
    'radius-sm',
    'radius-md',
    'huge',
    'radius-xl',
  ])
    expect(css, absent).not.toContain(absent);
  expect(themeCss(night({ colors: {}, fonts: {}, radius: {} }))).toBeUndefined();
});

test('the theme is one adopted style sheet, replaced in place and taken away again', () => {
  const page = new FakeSheet();
  adopted().push(page);
  applyThemeSheet(':root { --ink: #000; }');
  applyThemeSheet(':root { --ink: #111; }');
  expect(adopted()).toHaveLength(2);
  expect(adopted()[0]).toBe(page);
  expect(adopted()[1]!.text).toBe(':root { --ink: #111; }');
  applyThemeSheet(undefined);
  expect(adopted()).toEqual([page]);
});

test('the chosen theme is applied, read again when the window comes back, and taken away', async () => {
  const focus = () => act(async () => void window.dispatchEvent(new Event('focus')));
  read.mockResolvedValue({ ok: true, value: night() });
  const { rerender } = renderHook(({ name }) => usePluginTheme(name), {
    initialProps: { name: 'night' as string | null | undefined },
  });
  await act(async () => undefined);
  expect(read).toHaveBeenCalledWith({ name: 'night' });
  expect(shown()).toContain('--canvas: #1e1f1c;');

  // A theme being written shows its changes when the window comes back.
  read.mockResolvedValue({ ok: true, value: night({ colors: { canvas: '#101010' } }) });
  await focus();
  expect(shown()).toContain('--canvas: #101010;');

  // Core out of reach, or busy, says nothing about the theme: it stays.
  read.mockRejectedValue(new Error('gone'));
  await focus();
  read.mockResolvedValue({ ok: false, error: { message: 'ipc.timeout: Core 没有及时回答' } });
  await focus();
  expect(shown()).toContain('--canvas: #101010;');

  // Core says the plugin cannot give it now: Aura's own shows.
  read.mockResolvedValue({
    ok: false,
    error: { message: 'plugins.theme_unavailable: 插件 night 现在不能用' },
  });
  await focus();
  expect(shown()).toBe('');

  read.mockResolvedValue({ ok: true, value: night() });
  rerender({ name: 'night' });
  await focus();
  expect(shown()).not.toBe('');
  // None chosen: Aura's own, and the theme is not read again.
  rerender({ name: null });
  expect(shown()).toBe('');
  const reads = read.mock.calls.length;
  await focus();
  expect(read.mock.calls.length).toBe(reads);
});
