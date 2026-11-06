import { render } from '@testing-library/react';
import { expect, test } from 'vitest';

import { AppIcon } from '../app-icons';
import { ToolIcon } from '../tool-icon';
import { APP_ART } from './app-art';
import { SMALL_ART } from './app-art-small';
import { art, pixelIconSize, pixelPath, type PixelArt } from './pixel-art';
import { TOOL_ART } from './tool-art';

// Icons are drawn from pixel pictures, so that every square of the grid lands on whole screen
// pixels: a picture only ever fills its own grid, and is drawn at a whole multiple of it.

function fits(name: string, [top, rows]: PixelArt, grid: number) {
  expect(top, name).toBeGreaterThanOrEqual(0);
  expect(top + rows.length, name).toBeLessThanOrEqual(grid);
  for (const row of rows) {
    expect(row.length, name).toBeLessThanOrEqual(grid);
    expect(row, name).toMatch(/^[.#]*$/u);
  }
}

test('a picture keeps its rows as drawn, gap rows included', () => {
  expect(art(2, '\n.#\n\n#\n')).toEqual([2, ['.#', '', '#']]);
});

test('each run of lit pixels in a row is one rectangle a pixel high', () => {
  expect(pixelPath(art(2, '\n.##.#\n#\n'))).toBe('M1 2h2v1h-2zM4 2h1v1h-1zM0 3h1v1h-1z');
  expect(pixelPath([0, []])).toBe('');
});

test('every picture fits its grid and has something lit', () => {
  for (const [name, picture] of Object.entries(APP_ART)) {
    fits(name, picture, 16);
    expect(pixelPath(picture), name).not.toBe('');
  }
  for (const [name, picture] of Object.entries(SMALL_ART)) {
    fits(name, picture, 12);
    expect(pixelPath(picture), name).not.toBe('');
  }
  for (const [name, picture] of Object.entries(TOOL_ART)) {
    fits(name, picture, 16);
    // The loading icon is the pixel spinner, drawn by its style.
    if (name !== 'loader-circle') expect(pixelPath(picture), name).not.toBe('');
  }
});

test('a size asked for is drawn at the nearest whole multiple of the grid', () => {
  expect(pixelIconSize(10, true)).toEqual({ grid: 12, drawn: 12 });
  expect(pixelIconSize(12, true)).toEqual({ grid: 12, drawn: 12 });
  expect(pixelIconSize(10, false)).toEqual({ grid: 16, drawn: 16 });
  expect(pixelIconSize(14, true)).toEqual({ grid: 16, drawn: 16 });
  expect(pixelIconSize(22, false)).toEqual({ grid: 16, drawn: 16 });
  expect(pixelIconSize(24, false)).toEqual({ grid: 16, drawn: 32 });
  expect(pixelIconSize(28, true)).toEqual({ grid: 16, drawn: 32 });
});

test('icons fill with the text colour on crisp edges at their drawn size', () => {
  const { container } = render(
    <>
      <AppIcon className="small" name="close" size={11} />
      <AppIcon className="no-small" name="folder" size={12} />
      <AppIcon className="large" name="model" size={28} />
      <ToolIcon name="file-text" size={14} />
    </>,
  );
  const look = (selector: string) => {
    const svg = container.querySelector(selector)!;
    return [svg.getAttribute('viewBox'), svg.getAttribute('width'), svg.getAttribute('height')];
  };
  expect(look('.small')).toEqual(['0 0 12 12', '12', '12']);
  expect(look('.no-small')).toEqual(['0 0 16 16', '16', '16']);
  expect(look('.large')).toEqual(['0 0 16 16', '32', '32']);
  expect(look('.tool-icon')).toEqual(['0 0 16 16', '16', '16']);
  for (const svg of container.querySelectorAll('svg')) {
    expect(svg.getAttribute('fill')).toBe('currentColor');
    expect(svg.getAttribute('shape-rendering')).toBe('crispEdges');
    expect(svg.getAttribute('aria-hidden')).toBe('true');
    expect(svg.querySelector('path')!.getAttribute('d')).toMatch(/^(M\d+ \d+h\d+v1h-\d+z)+$/u);
  }
  expect(container.querySelector('.tool-icon')!.getAttribute('data-icon')).toBe('file-text');
  // On the 12-pixel grid it is the picture drawn for that grid, not the 16-pixel one.
  const drawn = (selector: string) =>
    container.querySelector(`${selector} path`)!.getAttribute('d');
  expect(drawn('.small')).toBe(pixelPath(SMALL_ART.close));
  expect(drawn('.no-small')).toBe(pixelPath(APP_ART.folder));
  expect(drawn('.large')).toBe(pixelPath(APP_ART.model));
  expect(drawn('.tool-icon')).toBe(pixelPath(TOOL_ART['file-text']));
});
