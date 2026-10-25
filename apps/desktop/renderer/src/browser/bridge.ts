import type { BrowserApiV1 } from '../../../shared/browser-api';

/** The preload's browser bridge, when this window has one. */
export const browserApi = (): BrowserApiV1 | undefined => window.aura?.v1.browser;
