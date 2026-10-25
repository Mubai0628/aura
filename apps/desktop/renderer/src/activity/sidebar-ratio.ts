/** Past this share of the window the sidebars leave the 近期活动 card no room beside the conversation. */
export const MAX_COMBINED_SIDEBAR_WIDTH_RATIO = 0.3;

/**
 * The ratio worth keeping when the sidebars are resized: the card only asks whether the sidebars take too much of the window, so the
 * kept value changes only when that answer does, and a drag re-renders nothing in between.
 */
export const keepSidebarRatio = (previous: number, next: number) =>
  previous >= MAX_COMBINED_SIDEBAR_WIDTH_RATIO === next >= MAX_COMBINED_SIDEBAR_WIDTH_RATIO
    ? previous
    : next;
