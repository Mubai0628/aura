const LEGACY_RIGHT_PANEL_WIDTH_KEY = 'aura.ui.v1.right-panel-width';
const WORKSPACE_LAYOUT_KEY = 'aura.ui.v2.workspace-layout';

export const DEFAULT_LEFT_PANEL_WIDTH = 250;
export const DEFAULT_RIGHT_PANEL_WIDTH = 324;
export const MIN_LEFT_PANEL_WIDTH = 200;
export const MIN_RIGHT_PANEL_WIDTH = 264;

export interface WorkspaceLayoutPreferences {
  leftPanelOpen: boolean;
  leftPanelWidth: number;
  rightPanelOpen: boolean;
  rightPanelWidth: number;
}

export const DEFAULT_WORKSPACE_LAYOUT_PREFERENCES: WorkspaceLayoutPreferences = {
  leftPanelOpen: true,
  leftPanelWidth: DEFAULT_LEFT_PANEL_WIDTH,
  rightPanelOpen: true,
  rightPanelWidth: DEFAULT_RIGHT_PANEL_WIDTH,
};

function validWidth(value: unknown, minimum: number, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= minimum
    ? Math.round(value)
    : fallback;
}

function readLegacyRightPanelWidth(): number {
  try {
    const stored = window.localStorage.getItem(LEGACY_RIGHT_PANEL_WIDTH_KEY);
    if (stored === null) return DEFAULT_RIGHT_PANEL_WIDTH;
    const parsed = Number.parseInt(stored, 10);
    return Number.isFinite(parsed) && parsed >= MIN_RIGHT_PANEL_WIDTH
      ? parsed
      : DEFAULT_RIGHT_PANEL_WIDTH;
  } catch {
    return DEFAULT_RIGHT_PANEL_WIDTH;
  }
}

export function readWorkspaceLayoutPreferences(): WorkspaceLayoutPreferences {
  try {
    const stored = window.localStorage.getItem(WORKSPACE_LAYOUT_KEY);
    if (stored === null) {
      return {
        ...DEFAULT_WORKSPACE_LAYOUT_PREFERENCES,
        rightPanelWidth: readLegacyRightPanelWidth(),
      };
    }
    const parsed: unknown = JSON.parse(stored);
    if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
      return DEFAULT_WORKSPACE_LAYOUT_PREFERENCES;
    }
    const record = parsed as Record<string, unknown>;
    return {
      leftPanelOpen:
        typeof record.leftPanelOpen === 'boolean'
          ? record.leftPanelOpen
          : DEFAULT_WORKSPACE_LAYOUT_PREFERENCES.leftPanelOpen,
      leftPanelWidth: validWidth(
        record.leftPanelWidth,
        MIN_LEFT_PANEL_WIDTH,
        DEFAULT_LEFT_PANEL_WIDTH,
      ),
      rightPanelOpen:
        typeof record.rightPanelOpen === 'boolean'
          ? record.rightPanelOpen
          : DEFAULT_WORKSPACE_LAYOUT_PREFERENCES.rightPanelOpen,
      rightPanelWidth: validWidth(
        record.rightPanelWidth,
        MIN_RIGHT_PANEL_WIDTH,
        DEFAULT_RIGHT_PANEL_WIDTH,
      ),
    };
  } catch {
    return DEFAULT_WORKSPACE_LAYOUT_PREFERENCES;
  }
}

export function writeWorkspaceLayoutPreferences(preferences: WorkspaceLayoutPreferences): void {
  try {
    window.localStorage.setItem(WORKSPACE_LAYOUT_KEY, JSON.stringify(preferences));
  } catch {
    // UI preferences are best-effort and never affect Core-owned state.
  }
}
