// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { setup, POLICY, S } from './page-test-support.js';

describe('browser pages', () => {
  it('start hidden on about:blank and load nothing before the settings are read', () => {
    const { pages, view, states } = setup();
    pages.open(S, 'tab-1');
    expect(view(0).visible).toBe(false);
    expect(view(0).webContents.loads).toEqual(['about:blank']);
    expect(states.at(-1)).toMatchObject({
      sessionId: S,
      pageId: 'tab-1',
      url: 'about:blank',
      closed: false,
    });
    expect(pages.navigate(S, 'tab-1', 'example.com')).toBe('内置浏览器已在设置中关闭');
    expect(view(0).webContents.loads).toEqual(['about:blank']);
  });

  it('open what the user typed unless the site is blocked', () => {
    const { pages, view, states } = setup();
    pages.setPolicy(POLICY);
    expect(pages.navigate(S, 'tab-1', 'example.com/a')).toBeUndefined();
    expect(view(0).webContents.loads.at(-1)).toBe('https://example.com/a');
    expect(pages.navigate(S, 'tab-1', 'https://blocked.example/')).toBe(
      'blocked.example 已在浏览器设置中被阻止',
    );
    expect(view(0).webContents.loads.at(-1)).toBe('https://example.com/a');
    expect(states.at(-1)?.notice).toEqual({
      kind: 'blocked',
      text: 'blocked.example 已在浏览器设置中被阻止',
    });
    expect(pages.navigate(S, 'tab-1', 'not an address')).toMatch(/请输入网址/u);
  });

  // Main gives a user's page the workspace of the
  // page it opened in it; that workspace's files load, nothing outside it, and never the model's.
  it("open a workspace page in the user's own page, with that workspace's files only", () => {
    const { pages, view } = setup();
    pages.setPolicy(POLICY);
    const root = 'file:///C:/Work/Game/';
    expect(pages.openFile(S, 'page-1', `${root}index.html`, root)).toBeUndefined();
    const contents = view(0).webContents;
    expect(contents.loads.at(-1)).toBe(`${root}index.html`);
    const request = (url: string, resourceType = 'mainFrame') =>
      pages.allowRequest({ webContentsId: contents.id, resourceType, url });
    expect(request(`${root}index.html`)).toBe(true);
    expect(request(`${root}levels/2.html`)).toBe(true);
    expect(request(`${root}game.js`, 'script')).toBe(true);
    expect(request('file:///C:/Users/me/.ssh/id_rsa', 'xmlhttprequest')).toBe(false);
    expect(request('file:///C:/Work/Other/x.html')).toBe(false);
    expect(pages.openFile(S, 'model', `${root}index.html`, root)).toBe(
      '只能在你自己的页面中打开工作区文件',
    );
    expect(pages.openFile(S, 'page-2', 'file:///C:/Work/Other/x.html', root)).toBe(
      '只能打开网页地址',
    );
  });

  it('show one page at a time, where the sidebar says', () => {
    const { pages, view } = setup();
    pages.setPolicy(POLICY);
    pages.show(S, 'tab-1', { x: 10, y: 20, width: 300, height: 200 });
    pages.show(S, 'tab-2', { x: 10, y: 20, width: 300, height: 200 });
    expect(view(0).visible).toBe(false);
    expect(view(1).visible).toBe(true);
    expect(view(1).bounds).toEqual({ x: 10, y: 20, width: 300, height: 200 });
    pages.hide(S, 'tab-2');
    expect(view(1).visible).toBe(false);
  });

  it("hold a read-only session's model page to allowed and approved sites, but not what the user typed", () => {
    const { pages, view, states } = setup();
    pages.setPolicy(POLICY);
    pages.open(S, 'model');
    const id = view(0).webContents.id;
    const main = (url: string) =>
      pages.allowRequest({ webContentsId: id, resourceType: 'mainFrame', url });
    expect(main('https://docs.rs/serde')).toBe(true);
    expect(main('https://other.example/')).toBe(false);
    view(0).webContents.fire(
      'did-fail-load',
      {},
      -20,
      'ERR_BLOCKED_BY_CLIENT',
      'https://other.example/',
      true,
    );
    expect(states.at(-1)?.notice).toEqual({
      kind: 'blocked',
      text: '只读会话打开 other.example 需要批准',
    });
    pages.setAccess(S, 'model', {
      kind: 'model',
      readOnly: true,
      approvedHosts: ['other.example'],
    });
    expect(main('https://other.example/')).toBe(true);
    pages.setAccess(S, 'model', { kind: 'model', readOnly: false, approvedHosts: [] });
    expect(main('https://third.example/')).toBe(true);
    expect(main('https://blocked.example/')).toBe(false);
    pages.setAccess(S, 'model', { kind: 'model', readOnly: true, approvedHosts: [] });
    pages.navigate(S, 'model', 'third.example');
    expect(main('https://third.example/')).toBe(true);
    view(0).webContents.fire('did-stop-loading');
    expect(main('https://third.example/')).toBe(false);
  });

  it('refuse blocked requests and local addresses asked for by a public page', () => {
    const { pages, view } = setup();
    pages.setPolicy(POLICY);
    pages.open(S, 'tab-1');
    const contents = view(0).webContents;
    contents.url = 'https://news.example/';
    const sub = (url: string) =>
      pages.allowRequest({ webContentsId: contents.id, resourceType: 'image', url });
    expect(sub('https://cdn.example/a.png')).toBe(true);
    expect(sub('https://px.tracker.example/p.gif')).toBe(false);
    expect(sub('http://192.168.1.1/admin')).toBe(false);
    contents.url = 'http://localhost:5173/';
    expect(sub('http://127.0.0.1:8080/api')).toBe(true);
    // A request with no page (a service worker's) still meets the block list.
    expect(
      pages.allowRequest({ resourceType: 'xmlhttprequest', url: 'https://blocked.example/x' }),
    ).toBe(false);
  });

  it('open a popup in the same page, under the same rules', () => {
    const { pages, view, states } = setup();
    pages.setPolicy(POLICY);
    pages.open(S, 'tab-1');
    const contents = view(0).webContents;
    expect(contents.openHandler?.({ url: 'https://docs.rs/' })).toEqual({ action: 'deny' });
    expect(contents.loads.at(-1)).toBe('https://docs.rs/');
    contents.openHandler?.({ url: 'https://blocked.example/' });
    expect(contents.loads.at(-1)).toBe('https://docs.rs/');
    expect(states.at(-1)?.notice?.kind).toBe('popup');
  });

  it('release the page used longest ago when too many are open, never the one on screen', () => {
    const { pages, states } = setup(2);
    pages.setPolicy(POLICY);
    pages.show(S, 'tab-1', { x: 0, y: 0, width: 10, height: 10 });
    pages.open(S, 'tab-2');
    pages.open(S, 'tab-3');
    const closed = states.filter((state) => state.closed);
    expect(closed.map((state) => state.pageId)).toEqual(['tab-2']);
    expect(closed[0]?.notice?.text).toMatch(/节省内存/u);
    expect(
      pages
        .states()
        .map((state) => state.pageId)
        .sort(),
    ).toEqual(['tab-1', 'tab-3']);
  });

  it('close every page when the browser is switched off, and a session takes its own pages', () => {
    const { pages, attached } = setup();
    pages.setPolicy(POLICY);
    pages.open(S, 'tab-1');
    pages.open('ses_other', 'tab-1');
    pages.closeSession(S);
    expect(pages.states().map((state) => state.sessionId)).toEqual(['ses_other']);
    pages.setPolicy({ ...POLICY, enabled: false });
    expect(pages.states()).toEqual([]);
    expect(attached.size).toBe(0);
  });

  it("treat the model page as the user's while the model waits for their help", () => {
    const { pages, view } = setup(2);
    pages.setPolicy(POLICY);
    pages.open(S, 'model');
    const id = view(0).webContents.id;
    const main = (url: string) =>
      pages.allowRequest({ webContentsId: id, resourceType: 'mainFrame', url });
    pages.setAccess(S, 'model', { kind: 'model', readOnly: true, approvedHosts: [] });
    expect(main('https://accounts.example/login')).toBe(false);
    pages.setUserControl(S, 'model', true);
    pages.setAssist(S, 'model', { reason: '请登录', deadline: 1 });
    // A sign-in that goes through another site is the user's navigation; the block list holds.
    expect(main('https://accounts.example/login')).toBe(true);
    expect(main('https://blocked.example/')).toBe(false);
    // A page waiting for the user is never released to make room.
    pages.open(S, 'tab-1');
    pages.open(S, 'tab-2');
    expect(pages.state(S, 'model')?.closed).toBe(false);
    // Answering hands the page back to the model, under the session's rules again.
    pages.setAssist(S, 'model', undefined);
    expect(pages.state(S, 'model')?.userInControl).toBe(false);
    expect(pages.state(S, 'model')?.assist).toBeUndefined();
    expect(main('https://accounts.example/login')).toBe(false);
  });

  it('let the user take over only the model page', () => {
    const { pages } = setup();
    pages.setPolicy(POLICY);
    pages.open(S, 'model');
    pages.open(S, 'tab-1');
    pages.setUserControl(S, 'model', true);
    pages.setUserControl(S, 'tab-1', true);
    expect(pages.state(S, 'model')?.userInControl).toBe(true);
    expect(pages.state(S, 'tab-1')?.userInControl).toBe(false);
  });
});
