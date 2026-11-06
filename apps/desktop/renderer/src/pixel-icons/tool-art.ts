/** The tool, state and action icons of a conversation's steps, on the 16-pixel grid. */
import { APP_ART } from './app-art';
import type { PixelArt } from './pixel-art';
import { STEP_TOOL_ART } from './tool-art-steps';
import { THING_TOOL_ART } from './tool-art-things';

export type ToolIconName =
  | 'activity'
  | 'check'
  | 'chevron-down'
  | 'chevron-right'
  | 'chevrons-down-up'
  | 'circle-alert'
  | 'circle-question-mark'
  | 'circle-slash'
  | 'copy'
  | 'corner-down-right'
  | 'external-link'
  | 'file-code'
  | 'file-diff'
  | 'file-pen-line'
  | 'file-search'
  | 'file-text'
  | 'folder-open'
  | 'folder-tree'
  | 'git-branch'
  | 'globe'
  | 'link'
  | 'list-checks'
  | 'list-end'
  | 'loader-circle'
  | 'minimize-2'
  | 'octagon-x'
  | 'package-minus'
  | 'package-plus'
  | 'package-search'
  | 'panel-right-open'
  | 'pause'
  | 'pencil'
  | 'play'
  | 'shield-alert'
  | 'sparkles'
  | 'square'
  | 'target'
  | 'terminal'
  | 'trash'
  | 'undo-2'
  | 'wrench';

/** The type fails when a name has no picture. */
export const TOOL_ART: Record<ToolIconName, PixelArt> = {
  activity: APP_ART.activity,
  check: APP_ART.check,
  'chevron-down': APP_ART['chevron-down'],
  'chevron-right': APP_ART['chevron-right'],
  'circle-question-mark': APP_ART.help,
  globe: APP_ART.browser,
  'minimize-2': APP_ART.shrink,
  sparkles: APP_ART.spark,
  terminal: APP_ART.terminal,
  wrench: APP_ART.wrench,
  ...THING_TOOL_ART,
  ...STEP_TOOL_ART,
};
