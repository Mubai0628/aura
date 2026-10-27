import type { ChromeControlApiV1 } from '../../../shared/chrome-control-api';

/** The preload's Chrome control bridge, when this window has one. */
export const chromeControl = (): ChromeControlApiV1 | undefined => window.aura?.v1.chrome;
