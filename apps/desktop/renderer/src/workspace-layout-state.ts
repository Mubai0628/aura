export const RIGHT_SIDEBAR_AUTO_HIDE_WIDTH = 480;
export const RIGHT_SIDEBAR_AUTO_RESTORE_WIDTH = 544;

export interface RightSidebarResponsiveState {
  autoHidden: boolean;
  narrowManualOverride: boolean;
}

export const DEFAULT_RIGHT_SIDEBAR_RESPONSIVE_STATE: RightSidebarResponsiveState = {
  autoHidden: false,
  narrowManualOverride: false,
};

export function respondToWindowWidth(
  current: RightSidebarResponsiveState,
  candidateConversationWidth: number,
  manuallyOpen: boolean,
): RightSidebarResponsiveState {
  if (candidateConversationWidth >= RIGHT_SIDEBAR_AUTO_RESTORE_WIDTH) {
    return DEFAULT_RIGHT_SIDEBAR_RESPONSIVE_STATE;
  }
  if (
    candidateConversationWidth < RIGHT_SIDEBAR_AUTO_HIDE_WIDTH &&
    manuallyOpen &&
    !current.narrowManualOverride
  ) {
    return { ...current, autoHidden: true };
  }
  return current;
}

export function manuallyOpenRightSidebar(
  candidateConversationWidth: number | undefined,
): RightSidebarResponsiveState {
  return {
    autoHidden: false,
    narrowManualOverride:
      candidateConversationWidth !== undefined &&
      candidateConversationWidth < RIGHT_SIDEBAR_AUTO_RESTORE_WIDTH,
  };
}
