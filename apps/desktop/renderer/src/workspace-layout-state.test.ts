import { describe, expect, test } from 'vitest';

import { rightSidebarNeedsFullArea } from './workspace-layout-state';

describe('right sidebar layout', () => {
  test('fills the main area when a split would squeeze the conversation below its minimum', () => {
    expect(rightSidebarNeedsFullArea(false, 480)).toBe(false);
    expect(rightSidebarNeedsFullArea(false, 479)).toBe(true);
  });

  test('restores the split only once the main area has sufficient room again', () => {
    expect(rightSidebarNeedsFullArea(true, 543)).toBe(true);
    expect(rightSidebarNeedsFullArea(true, 544)).toBe(false);
  });
});
