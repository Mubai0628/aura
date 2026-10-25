import type { BrowserWindow, WebContents } from 'electron';

/**
 * The window's contents while the window is alive. A destroyed BrowserWindow throws on
 * `.webContents`; terminal output timers and Core notifications that land while Aura quits must
 * find no receiver instead. An uncaught throw in Main shows a modal error box that keeps Aura
 * from exiting until someone clicks it.
 */
export const liveContents = (window: BrowserWindow | undefined): WebContents | undefined =>
  window !== undefined && !window.isDestroyed() ? window.webContents : undefined;
