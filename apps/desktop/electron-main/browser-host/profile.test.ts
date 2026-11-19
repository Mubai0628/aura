// @vitest-environment node
import { describe, expect, it, vi } from 'vitest';

import { BrowserPages, type PageViews } from './pages.js';
import { browserUserAgent, guardProfile, type ProfileSession } from './profile.js';
import { parsedRules } from './site-rules.js';

function fakeSession() {
  const calls = {
    permission: undefined as
      undefined | Parameters<ProfileSession['setPermissionRequestHandler']>[0],
    check: undefined as undefined | (() => boolean),
    device: undefined as undefined | (() => boolean),
    download: undefined as undefined | Parameters<ProfileSession['on']>[1],
    request: undefined as
      undefined | Parameters<ProfileSession['webRequest']['onBeforeRequest']>[0],
    userAgent: '',
  };
  const session: ProfileSession = {
    setPermissionRequestHandler: (handler) => (calls.permission = handler),
    setPermissionCheckHandler: (handler) => (calls.check = handler),
    setDevicePermissionHandler: (handler) => (calls.device = handler),
    getUserAgent: () =>
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) @aura/desktop/0.1.0 Chrome/144.0.0.0 Electron/44.0.0 Safari/537.36',
    setUserAgent: (value) => (calls.userAgent = value),
    on: (_event, listener) => (calls.download = listener),
    webRequest: { onBeforeRequest: (listener) => (calls.request = listener) },
  };
  return { session, calls };
}

const views: PageViews = {
  create: () => {
    throw new Error('no pages in this test');
  },
  attach: () => undefined,
  detach: () => undefined,
};

describe('the browser profile', () => {
  it('grants no permission and checks none as granted', () => {
    const { session, calls } = fakeSession();
    guardProfile(session, new BrowserPages(views));
    const answers: boolean[] = [];
    for (const permission of [
      'media',
      'geolocation',
      'notifications',
      'clipboard-read',
      'openExternal',
    ])
      calls.permission?.(undefined, permission, (granted) => answers.push(granted));
    expect(answers).toEqual([false, false, false, false, false]);
    expect(calls.check?.()).toBe(false);
    expect(calls.device?.()).toBe(false);
  });

  it('stops every download, and the page says why and what to do instead', () => {
    const { session, calls } = fakeSession();
    const pages = new BrowserPages(views);
    guardProfile(session, pages);
    let prevented = false;
    calls.download?.(
      { preventDefault: () => (prevented = true) },
      { getFilename: () => 'setup.exe' },
      undefined,
    );
    expect(prevented).toBe(true);
    const notice = vi.spyOn(pages, 'notice');
    calls.download?.(
      { preventDefault: () => undefined },
      { getFilename: () => 'report.pdf' },
      { id: 7 },
    );
    expect(notice).toHaveBeenCalledWith(
      7,
      'download',
      '内置浏览器不支持下载，report.pdf 没有下载；可以把这个页面的地址复制到系统浏览器里打开，再点一次下载。',
      'report.pdf',
    );
  });

  it('sends every request through the site policy', () => {
    const { session, calls } = fakeSession();
    const pages = new BrowserPages(views);
    guardProfile(session, pages);
    const cancelled: boolean[] = [];
    const ask = (url: string) =>
      calls.request?.({ resourceType: 'script', url }, (response) =>
        cancelled.push(response.cancel),
      );
    ask('https://cdn.example/a.js');
    pages.setPolicy({ enabled: true, allowed: [], blocked: parsedRules(['cdn.example']) });
    ask('https://cdn.example/a.js');
    ask('https://other.example/a.js');
    // Nothing loads before the settings are read; then only the blocked site is refused.
    expect(cancelled).toEqual([true, true, false]);
  });

  it('presents an ordinary Chrome user agent', () => {
    const { session, calls } = fakeSession();
    guardProfile(session, new BrowserPages(views));
    expect(calls.userAgent).toBe(
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/144.0.0.0 Safari/537.36',
    );
    expect(browserUserAgent('X Aura/1.2 Y')).toBe('X Y');
  });
});
