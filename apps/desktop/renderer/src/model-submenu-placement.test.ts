import { expect, test } from 'vitest';
import { placeSubmenu, shiftInside } from './model-submenu-placement';

// In a window that was not full screen, the open provider's models went left of the providers past
// the conversation column, which cut them off under the side panel. They now go where the column
// has room: left, right, narrowed, or above the providers. The boxes are those measured in
// development Aura with the right panel open.

const box = (left: number, right: number, top = 560, bottom = 700) => ({
  left,
  right,
  top,
  bottom,
});
const column = (left: number, right: number) => box(left, right, 40, 860);

test('the models go left of the providers when the column has room for them there', () => {
  expect(placeSubmenu(box(700, 910), column(258, 1080))).toEqual({ side: 'left', width: 280 });
});

test('with room only on the right, they go right', () => {
  expect(placeSubmenu(box(300, 510), column(258, 900))).toEqual({ side: 'right', width: 280 });
});

test('a little short of room, they narrow on the roomier side instead of leaving the column', () => {
  // A 1386 px window: the column is 258..814 and the providers 506..716.
  expect(placeSubmenu(box(506, 716), column(258, 814))).toEqual({ side: 'left', width: 234 });
});

test('with room on neither side, they go above the providers, inside the column', () => {
  // A 986 px window: the column is 258..614 and the providers 319..529.
  expect(placeSubmenu(box(319, 529), column(258, 614))).toEqual({
    side: 'above',
    width: 280,
    shift: 17,
    maxHeight: 506,
  });
  // An 886 px window: a column of 258..564, narrower than the models list with its margins.
  expect(placeSubmenu(box(273, 483), column(258, 564))).toEqual({
    side: 'above',
    width: 280,
    shift: 63,
    maxHeight: 506,
  });
  expect(placeSubmenu(box(270, 470), column(258, 520))).toEqual({
    side: 'above',
    width: 246,
    shift: 42,
    maxHeight: 506,
  });
});

test('the providers move right to stay inside a narrow column, as far as it has room', () => {
  // An 886 px window: the providers end at the model button, 449, and the column starts at 258.
  expect(shiftInside(239, 449, column(258, 564))).toBe(27);
  expect(shiftInside(472, 682, column(258, 814))).toBe(0);
  // No room on the right either: they stay at the model button.
  expect(shiftInside(239, 560, column(258, 564))).toBe(0);
});
