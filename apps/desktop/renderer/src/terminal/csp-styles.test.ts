import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { adoptTerminalStyles } from './csp-styles';

// jsdom has no constructed stylesheets; this stands in for the browser's.
class FakeSheet {
  static replaced = 0;
  text = '';
  replaceSync(text: string) {
    FakeSheet.replaced++;
    this.text = text;
  }
}
const adopted = () =>
  (document as unknown as { adoptedStyleSheets: FakeSheet[] }).adoptedStyleSheets;
const texts = () => adopted().map((sheet) => sheet.text);
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const style = (text: string) => {
  const element = document.createElement('style');
  element.textContent = text;
  return element;
};

beforeEach(() => {
  FakeSheet.replaced = 0;
  vi.stubGlobal('CSSStyleSheet', FakeSheet);
  Object.defineProperty(document, 'adoptedStyleSheets', {
    value: [],
    writable: true,
    configurable: true,
  });
});
afterEach(() => {
  vi.unstubAllGlobals();
  delete (document as { adoptedStyleSheets?: unknown }).adoptedStyleSheets;
});

it('mirrors the screen styles CSP blocks, follows rewrites, and ignores row updates', async () => {
  const page = new FakeSheet();
  adopted().push(page);
  const screen = document.createElement('div');
  const rows = document.createElement('div');
  const theme = style('.owner-1 .xterm-rows { color: #111; }');
  screen.append(rows, theme);
  const release = adoptTerminalStyles(screen);
  expect(texts()).toEqual(['', '.owner-1 .xterm-rows { color: #111; }']);

  // The cell style is created on the first resize, after the terminal opened.
  const cells = style('.owner-1 .xterm-rows span { display: inline-block; }');
  screen.append(cells);
  await settle();
  expect(texts()).toEqual([
    '',
    '.owner-1 .xterm-rows { color: #111; }',
    '.owner-1 .xterm-rows span { display: inline-block; }',
  ]);

  theme.textContent = '.owner-1 .xterm-rows { color: #222; }';
  await settle();
  expect(texts()[1]).toBe('.owner-1 .xterm-rows { color: #222; }');

  // Output frames rewrite rows below the screen; the same cell text is a resize with no change.
  const replaced = FakeSheet.replaced;
  for (let index = 0; index < 20; index++) {
    const row = document.createElement('div');
    row.textContent = `line ${index}`;
    rows.replaceChildren(row);
  }
  cells.textContent = '.owner-1 .xterm-rows span { display: inline-block; }';
  await settle();
  expect(FakeSheet.replaced).toBe(replaced);

  theme.remove();
  await settle();
  expect(texts()).toEqual(['', '.owner-1 .xterm-rows span { display: inline-block; }']);

  release();
  expect(adopted()).toEqual([page]);
  screen.append(style('.owner-1 .late { color: red; }'));
  await settle();
  expect(adopted()).toEqual([page]);
});
