/**
 * The browser's own persistent profile. Sign-ins survive a restart and stay apart from
 * Aura's interface. No page may use the camera, microphone, location, notifications, the
 * clipboard or devices; downloads are stopped; every request meets the site policy.
 */
import type { BrowserPages, BrowserRequest } from './pages.js';

export const BROWSER_PARTITION = 'persist:aura-browser';

/** The parts of Electron's Session the profile sets; tests pass a fake. */
export interface ProfileSession {
  setPermissionRequestHandler(
    handler: (contents: unknown, permission: string, callback: (granted: boolean) => void) => void,
  ): void;
  setPermissionCheckHandler(handler: () => boolean): void;
  setDevicePermissionHandler(handler: () => boolean): void;
  getUserAgent(): string;
  setUserAgent(userAgent: string): void;
  on(
    event: 'will-download',
    listener: (
      event: { preventDefault(): void },
      item: { getFilename(): string },
      contents: { id: number } | undefined,
    ) => void,
  ): unknown;
  readonly webRequest: {
    onBeforeRequest(
      listener: (
        request: BrowserRequest,
        callback: (response: { cancel: boolean }) => void,
      ) => void,
    ): void;
  };
}

/** Pages see an ordinary Chrome: some sites refuse an "Electron" or "Aura" user agent. */
export function browserUserAgent(electronUserAgent: string): string {
  return electronUserAgent.replace(/ (?:Electron|aura|Aura|@aura\/desktop)\/\S+/gu, '');
}

export function guardProfile(session: ProfileSession, pages: BrowserPages): void {
  session.setPermissionRequestHandler((_contents, _permission, callback) => callback(false));
  session.setPermissionCheckHandler(() => false);
  session.setDevicePermissionHandler(() => false);
  session.setUserAgent(browserUserAgent(session.getUserAgent()));
  session.on('will-download', (event, item, contents) => {
    // Downloads are not part of this browser yet; the model has http.download for files.
    event.preventDefault();
    if (contents !== undefined)
      pages.notice(contents.id, 'download', `已阻止下载 ${item.getFilename()}`);
  });
  session.webRequest.onBeforeRequest((request, callback) =>
    callback({ cancel: !pages.allowRequest(request) }),
  );
}
