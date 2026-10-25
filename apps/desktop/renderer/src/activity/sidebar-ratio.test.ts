import { expect, test } from 'vitest';
import { MAX_COMBINED_SIDEBAR_WIDTH_RATIO, keepSidebarRatio } from './sidebar-ratio';

// Every drag frame used to re-render the whole conversation through this one number.
test('a resize keeps the ratio until the card would change, then takes the new one', () => {
  const below = MAX_COMBINED_SIDEBAR_WIDTH_RATIO - 0.1;
  const above = MAX_COMBINED_SIDEBAR_WIDTH_RATIO + 0.1;
  expect(keepSidebarRatio(below, below + 0.05)).toBe(below);
  expect(keepSidebarRatio(below, above)).toBe(above);
  expect(keepSidebarRatio(above, above + 0.2)).toBe(above);
  expect(keepSidebarRatio(above, below)).toBe(below);
});
