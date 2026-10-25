import { describe, expect, test } from 'vitest';

import {
  DEFAULT_RIGHT_SIDEBAR_RESPONSIVE_STATE,
  manuallyOpenRightSidebar,
  respondToWindowWidth,
} from './workspace-layout-state';

describe('right sidebar responsive state', () => {
  test('auto-hides below 480px and retains that state inside the hysteresis band', () => {
    const hidden = respondToWindowWidth(DEFAULT_RIGHT_SIDEBAR_RESPONSIVE_STATE, 479, true);
    expect(hidden).toEqual({ autoHidden: true, narrowManualOverride: false });
    expect(respondToWindowWidth(hidden, 520, true)).toBe(hidden);
  });

  test('only restores at 544px and does not open a manually closed sidebar', () => {
    const hidden = { autoHidden: true, narrowManualOverride: false };
    expect(respondToWindowWidth(hidden, 543, true)).toBe(hidden);
    expect(respondToWindowWidth(hidden, 544, true)).toEqual(DEFAULT_RIGHT_SIDEBAR_RESPONSIVE_STATE);
    expect(respondToWindowWidth(DEFAULT_RIGHT_SIDEBAR_RESPONSIVE_STATE, 300, false)).toBe(
      DEFAULT_RIGHT_SIDEBAR_RESPONSIVE_STATE,
    );
  });

  test('keeps a narrow manual reopen visible until the restore band is crossed', () => {
    const reopened = manuallyOpenRightSidebar(479);
    expect(reopened).toEqual({ autoHidden: false, narrowManualOverride: true });
    expect(respondToWindowWidth(reopened, 300, true)).toBe(reopened);
    expect(respondToWindowWidth(reopened, 544, true)).toEqual(
      DEFAULT_RIGHT_SIDEBAR_RESPONSIVE_STATE,
    );
  });
});
