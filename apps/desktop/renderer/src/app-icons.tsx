import type { SVGProps } from 'react';
import { APP_ART } from './pixel-icons/app-art';
import { SMALL_ART } from './pixel-icons/app-art-small';
import type { PixelArt } from './pixel-icons/pixel-art';
import { PixelIcon } from './pixel-icons/PixelIcon';

export type IconName =
  | 'activity'
  | 'archive'
  | 'arrow-down'
  | 'arrow-left'
  | 'arrow-right'
  | 'bell'
  | 'bolt'
  | 'book'
  | 'bookmark'
  | 'browser'
  | 'chat'
  | 'check'
  | 'chevron-down'
  | 'chevron-left'
  | 'chevron-right'
  | 'close'
  | 'code'
  | 'desktop'
  | 'expand'
  | 'file'
  | 'folder'
  | 'hand'
  | 'help'
  | 'home'
  | 'eye'
  | 'eye-off'
  | 'model'
  | 'plug'
  | 'minus'
  | 'more'
  | 'plus'
  | 'puzzle'
  | 'review'
  | 'refresh'
  | 'search'
  | 'send'
  | 'settings'
  | 'shield'
  | 'shrink'
  | 'slash'
  | 'sidebar-left'
  | 'sidebar-right'
  | 'spark'
  | 'terminal'
  | 'warning'
  | 'workspace'
  | 'wrench';

interface AppIconProps extends Omit<SVGProps<SVGSVGElement>, 'children' | 'name'> {
  name: IconName;
  size?: number;
}

const SMALL: Partial<Record<IconName, PixelArt>> = SMALL_ART;

/** One of the window's pixel icons; `size` is the size asked for, drawn at the nearest whole one. */
export function AppIcon({ name, size = 16, ...props }: AppIconProps) {
  return <PixelIcon art={APP_ART[name]} size={size} small={SMALL[name]} {...props} />;
}
