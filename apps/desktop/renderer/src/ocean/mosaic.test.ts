import { expect, test } from 'vitest';
import { CELL, Mosaic } from './mosaic';
import { Swimmer } from './swimmer';

// A shark is hundreds of small shaded tiles, each
// with a character, like the video.

/** The scene's widest shark, heading right with its body laid out to the left along row 30. */
const SPINE_ROW = 30;
function shark(kind: 'white' | 'leopard' = 'white') {
  const still = { phase: -Math.PI / 2, rx: 0, ry: 0, speed: 0, pace: 0, wobble: 0 };
  const swimmer = new Swimmer(kind, 1100, still);
  swimmer.place(1000, (SPINE_ROW + 0.5) * CELL);
  return swimmer;
}

test('a shark is hundreds of tiles, each with a colour and a character, all inside the grid', () => {
  const mosaic = new Mosaic(100, 60);
  const tiles = mosaic.tiles([shark()]);
  expect(tiles.length).toBeGreaterThan(150);
  // Dense: at its widest the body alone is several tiles across, the fins wider still.
  const rowsAt = (col: number) => tiles.filter((tile) => tile.col === col).length;
  expect(Math.max(...tiles.map((tile) => rowsAt(tile.col)))).toBeGreaterThanOrEqual(7);
  for (const tile of tiles) {
    expect(tile.col).toBeGreaterThanOrEqual(0);
    expect(tile.col).toBeLessThan(100);
    expect(tile.row).toBeLessThan(60);
    expect(tile.fill).toMatch(/^rgb\(\d+,\d+,\d+\)$/u);
    expect(tile.glyph).toHaveLength(1);
    expect(tile.alpha).toBeGreaterThan(0);
    expect(tile.alpha).toBeLessThanOrEqual(1);
  }
  // Shaded like a rounded body, not one flat colour.
  expect(new Set(tiles.map((tile) => tile.fill)).size).toBeGreaterThan(40);
  expect(new Set(tiles.map((tile) => tile.glyph)).size).toBeGreaterThan(3);
});

test('its spine is darker than its flanks, and a leopard shark has spots', () => {
  const mosaic = new Mosaic(100, 60);
  const tiles = mosaic.tiles([shark()]);
  const lum = (fill: string) => {
    const [r, g, b] = fill.match(/\d+/gu)!.map(Number) as [number, number, number];
    return r * 0.3 + g * 0.59 + b * 0.11;
  };
  // The tiles in the row of the spine (it runs left from the head) against the body's edge rows.
  const middle = tiles.filter((tile) => tile.row === SPINE_ROW && tile.alpha === 1);
  const edges = tiles.filter((tile) => Math.abs(tile.row - SPINE_ROW) === 2 && tile.alpha === 1);
  const average = (list: typeof tiles) =>
    list.reduce((sum, tile) => sum + lum(tile.fill), 0) / list.length;
  expect(middle.length).toBeGreaterThan(10);
  expect(edges.length).toBeGreaterThan(10);
  expect(average(middle)).toBeLessThan(average(edges));

  const spotted = new Mosaic(100, 60).tiles([shark('leopard')]);
  expect(spotted.some((tile) => lum(tile.fill) < 70)).toBe(true);
});

test('an empty sea lays no tiles', () => {
  expect(new Mosaic(20, 20).tiles([])).toEqual([]);
});
