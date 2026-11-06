import type { IconName } from '../app-icons';
import { CONTROL_ART } from './app-art-controls';
import { OBJECT_ART } from './app-art-objects';
import type { PixelArt } from './pixel-art';

/** Every icon of the window on the 16-pixel grid; the type fails when one has no picture. */
export const APP_ART: Record<IconName, PixelArt> = { ...CONTROL_ART, ...OBJECT_ART };
