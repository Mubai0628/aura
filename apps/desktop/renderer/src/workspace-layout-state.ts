export const MIN_CONVERSATION_PANE_WIDTH = 480;
const SPLIT_RESTORE_MARGIN = 64;

export function rightSidebarNeedsFullArea(
  wasCompact: boolean,
  availableConversationWidth: number,
): boolean {
  const minimum = MIN_CONVERSATION_PANE_WIDTH + (wasCompact ? SPLIT_RESTORE_MARGIN : 0);
  return availableConversationWidth < minimum;
}
