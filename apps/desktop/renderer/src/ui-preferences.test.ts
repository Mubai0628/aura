import { beforeEach, describe, expect, test } from 'vitest';

import {
  DEFAULT_WORKSPACE_LAYOUT_PREFERENCES,
  readWorkspaceLayoutPreferences,
  writeWorkspaceLayoutPreferences,
} from './ui-preferences';

describe('workspace layout preferences', () => {
  beforeEach(() => window.localStorage.clear());

  test('round-trips manual intent and preferred widths', () => {
    const preferences = {
      leftPanelOpen: false,
      leftPanelWidth: 286,
      rightPanelOpen: true,
      rightPanelWidth: 418,
    };
    writeWorkspaceLayoutPreferences(preferences);
    expect(readWorkspaceLayoutPreferences()).toEqual(preferences);
  });

  test('rejects malformed values without importing responsive state', () => {
    window.localStorage.setItem(
      'aura.ui.v2.workspace-layout',
      JSON.stringify({
        leftPanelOpen: 'false',
        leftPanelWidth: 12,
        rightPanelOpen: null,
        rightPanelWidth: Number.NaN,
        rightSidebarAutoHidden: true,
      }),
    );
    expect(readWorkspaceLayoutPreferences()).toEqual(DEFAULT_WORKSPACE_LAYOUT_PREFERENCES);
  });

  test('imports the previous right width once when no v2 preference exists', () => {
    window.localStorage.setItem('aura.ui.v1.right-panel-width', '390');
    expect(readWorkspaceLayoutPreferences()).toEqual({
      ...DEFAULT_WORKSPACE_LAYOUT_PREFERENCES,
      rightPanelWidth: 390,
    });
  });
});
