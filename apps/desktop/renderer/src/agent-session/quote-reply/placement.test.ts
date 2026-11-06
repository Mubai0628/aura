import { expect, test } from 'vitest';
import { placeBeside } from './placement';

// The 回复 button and the reply card sit beside the selection, on the side that has room inside
// the part of the conversation that can be seen.

const area = { left: 100, top: 50, right: 900, bottom: 650 };
const size = { width: 60, height: 26 };
const box = (left: number, top: number, right: number, bottom: number) => ({
  left,
  top,
  right,
  bottom,
});

test('above the selection and centred on it when there is room above', () => {
  expect(placeBeside(box(300, 200, 500, 240), size, area, 'center')).toEqual({
    left: 370,
    top: 168,
  });
});

test('below the selection when there is no room above it', () => {
  expect(placeBeside(box(300, 60, 500, 100), size, area, 'center')).toEqual({
    left: 370,
    top: 106,
  });
});

test('over the selection when it fills what can be seen', () => {
  expect(placeBeside(box(300, 40, 500, 660), size, area, 'center')).toEqual({
    left: 370,
    top: 56,
  });
});

test('a card starts at the selection’s left edge and stays inside what can be seen', () => {
  const card = { width: 380, height: 120 };
  expect(placeBeside(box(300, 400, 500, 440), card, area, 'start')).toEqual({
    left: 300,
    top: 274,
  });
  expect(placeBeside(box(700, 400, 880, 440), card, area, 'start')).toEqual({
    left: 512,
    top: 274,
  });
  expect(placeBeside(box(20, 400, 200, 440), card, area, 'start')).toEqual({
    left: 108,
    top: 274,
  });
});

test('nothing when the selection is out of sight; a card that stays goes to the nearest edge', () => {
  expect(placeBeside(box(300, -200, 500, -160), size, area, 'center')).toBeUndefined();
  expect(placeBeside(box(300, 700, 500, 740), size, area, 'center')).toBeUndefined();
  expect(placeBeside(box(300, -200, 500, -160), size, area, 'center', true)).toEqual({
    left: 370,
    top: 56,
  });
  expect(placeBeside(box(300, 700, 500, 740), size, area, 'center', true)).toEqual({
    left: 370,
    top: 618,
  });
});
