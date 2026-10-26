/**
 * The workspaces open in the sidebar. The sidebar is not rendered while Settings is open, so its
 * own state would start over on the way back; this window keeps the set, like the layout and the
 * open conversation, so it also survives a restart.
 */
export const SIDEBAR_EXPANDED_KEY = 'aura.ui.v1.sidebar-expanded';

/** The workspaces kept open, or `undefined` when nothing usable is stored. */
export function readExpandedWorkspaces(): string[] | undefined {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(SIDEBAR_EXPANDED_KEY) ?? 'null');
    return Array.isArray(value) && value.every((id) => typeof id === 'string') ? value : undefined;
  } catch {
    return undefined;
  }
}

export function rememberExpandedWorkspaces(ids: readonly string[]): void {
  try {
    localStorage.setItem(SIDEBAR_EXPANDED_KEY, JSON.stringify(ids));
  } catch {
    /* Not kept: the sidebar opens only the selected workspace next time. */
  }
}
