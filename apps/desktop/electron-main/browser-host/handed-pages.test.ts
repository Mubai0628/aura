// @vitest-environment node
import type { BrowserHostActionV1, BrowserHostCommandV1 } from '@aura/contracts/generated';
import { describe, expect, it, vi } from 'vitest';

import { execute } from './actions.js';
import { Assists } from './assist.js';
import { command, Contents, S, setup } from './executor-fakes.js';
import { BrowserPages, type PageView } from './pages.js';

/** The model's command on the page the user handed over as `page`. */
const on = (
  page: string,
  action: BrowserHostActionV1,
  expected: string | null = null,
): BrowserHostCommandV1 => ({ ...command(action, expected), page });

const look: BrowserHostActionV1 = { kind: 'snapshot', max_chars: 12_000 };
const click: BrowserHostActionV1 = {
  kind: 'click',
  element: null,
  x: 10,
  y: 10,
  button: 'left',
  double: false,
};

/** The user's own page, at the address they typed. */
function userPage(pages: BrowserPages, id: string, url: string): void {
  pages.open(S, id);
  expect(pages.navigate(S, id, url)).toBeUndefined();
}

describe('a page the user hands to the model', () => {
  it("is named p1, p2… in its conversation, once each; the model's own page and a closed page are not handed", () => {
    const { pages } = setup();
    userPage(pages, 'page-a', 'http://127.0.0.1:9/a');
    userPage(pages, 'page-b', 'http://127.0.0.1:9/b');
    expect(pages.handOver(S, 'page-a')).toBe('p1');
    expect(pages.handOver(S, 'page-a')).toBe('p1');
    expect(pages.handOver(S, 'page-b')).toBe('p2');
    expect(pages.state(S, 'page-a')).toMatchObject({ handedAs: 'p1' });
    expect(pages.handed(S, 'p2')).toBe('page-b');
    expect(pages.handOver(S, 'model')).toBeUndefined();
    expect(pages.handOver(S, 'page-never-opened')).toBeUndefined();
    expect(pages.state(S, 'page-never-opened')).toBeUndefined();
    // A closed page's id names no other page later.
    pages.close(S, 'page-a');
    expect(pages.handed(S, 'p1')).toBeUndefined();
    userPage(pages, 'page-c', 'http://127.0.0.1:9/c');
    expect(pages.handOver(S, 'page-c')).toBe('p3');
    // Another conversation's ids are its own.
    expect(pages.handed('ses_01ARZ3NDEKTSV4RRFFQ69G5FAW', 'p2')).toBeUndefined();
  });

  it("takes the model's commands that name it, and refuses an id that names no handed page", async () => {
    const { deps, pages } = setup();
    userPage(pages, 'page-a', 'http://127.0.0.1:9/a');
    pages.handOver(S, 'page-a');
    const seen = await execute(deps, on('p1', look));
    expect(seen.page_url).toBe('http://127.0.0.1:9/a');
    expect(seen.outcome).toMatchObject({
      outcome: 'completed',
      result: { url: 'http://127.0.0.1:9/a', notice: expect.stringContaining('交给你') },
    });
    // The model's own page was never opened for it.
    expect(pages.state(S, 'model')).toBeUndefined();
    for (const page of ['p2', 'page-a'])
      expect((await execute(deps, on(page, look))).outcome).toMatchObject({
        outcome: 'refused',
        message: expect.stringContaining(page),
      });
  });

  it('takes input only once the model has looked at it', async () => {
    const { deps, pages, tools } = setup();
    userPage(pages, 'page-a', 'http://127.0.0.1:9/a');
    pages.handOver(S, 'page-a');
    expect((await execute(deps, on('p1', click, '127.0.0.1'))).outcome).toMatchObject({
      outcome: 'refused',
      message: expect.stringContaining('browser.snapshot'),
    });
    expect(tools.methods()).not.toContain('Input.dispatchMouseEvent');
    await execute(deps, on('p1', look));
    expect((await execute(deps, on('p1', click, '127.0.0.1'))).outcome.outcome).toBe('completed');
    expect(tools.methods()).toContain('Input.dispatchMouseEvent');
  });

  it("is taken over and given back as the model's page is; a page never handed over has no takeover", async () => {
    const { deps, pages } = setup();
    userPage(pages, 'page-a', 'http://127.0.0.1:9/a');
    pages.handOver(S, 'page-a');
    await execute(deps, on('p1', look));
    pages.setUserControl(S, 'page-a', true);
    expect(pages.state(S, 'page-a')).toMatchObject({ userInControl: true });
    expect((await execute(deps, on('p1', click, '127.0.0.1'))).outcome).toMatchObject({
      outcome: 'refused',
      message: expect.stringContaining('已接管'),
    });
    pages.setUserControl(S, 'page-a', false);
    expect((await execute(deps, on('p1', click, '127.0.0.1'))).outcome).toMatchObject({
      outcome: 'refused',
      message: expect.stringContaining('browser.snapshot'),
    });
    await execute(deps, on('p1', look));
    expect((await execute(deps, on('p1', click, '127.0.0.1'))).outcome.outcome).toBe('completed');
    userPage(pages, 'page-b', 'http://127.0.0.1:9/b');
    pages.setUserControl(S, 'page-b', true);
    expect(pages.state(S, 'page-b')).toMatchObject({ userInControl: false });
  });

  it('is never closed by the model, and stays open while other pages are released to save memory', async () => {
    const { deps, pages } = setup();
    userPage(pages, 'page-a', 'http://127.0.0.1:9/a');
    pages.handOver(S, 'page-a');
    expect((await execute(deps, on('p1', { kind: 'close' }))).outcome).toMatchObject({
      outcome: 'refused',
      message: expect.stringContaining('不能关闭'),
    });
    expect(pages.state(S, 'page-a')).toMatchObject({ closed: false });

    const views = {
      create: () =>
        ({ webContents: new Contents(), setBounds() {}, setVisible() {} }) as unknown as PageView,
      attach() {},
      detach() {},
    };
    const few = new BrowserPages(views, { limit: 2 });
    few.setPolicy({ enabled: true, allowed: [], blocked: [] });
    few.open(S, 'page-a');
    few.handOver(S, 'page-a');
    few.open(S, 'page-b');
    few.open(S, 'page-c');
    expect(few.state(S, 'page-a')).toMatchObject({ closed: false, handedAs: 'p1' });
    expect(few.state(S, 'page-b')).toMatchObject({ closed: true });
  });

  it("navigates by the conversation's rules when the model moves it, and by the user's own when the user does", () => {
    const { pages } = setup();
    userPage(pages, 'page-a', 'http://127.0.0.1:9/a');
    pages.handOver(S, 'page-a');
    pages.setAccess(S, 'page-a', { kind: 'model', readOnly: true, approvedHosts: [] });
    const contents = pages.contents(S, 'page-a');
    const loads = (url: string) =>
      pages.allowRequest({ webContentsId: contents?.id, resourceType: 'mainFrame', url });
    // Moved by the model in a ReadOnly conversation: allowed and approved sites only.
    void pages.loadForModel(S, 'page-a', 'https://other.example/');
    expect(loads('https://other.example/')).toBe(false);
    // While the user has it, a link they follow is theirs.
    pages.setUserControl(S, 'page-a', true);
    expect(loads('https://third.example/')).toBe(true);
    pages.setUserControl(S, 'page-a', false);
    expect(loads('https://third.example/')).toBe(false);
    // An address the user types is theirs too.
    expect(pages.navigate(S, 'page-a', 'https://other.example/')).toBeUndefined();
    expect(loads('https://other.example/')).toBe(true);
  });

  it("asks for the user's help on it and is drawn as the model's page", async () => {
    const emulated: string[] = [];
    const { deps, pages } = setup({
      emulate: async (_session, page) => {
        emulated.push(page);
        return 0.5;
      },
    });
    const assists = new Assists(pages);
    deps.assist = (session, page, id, reason, timeoutMs) =>
      assists.wait(session, id, reason, timeoutMs, page);
    userPage(pages, 'page-a', 'http://127.0.0.1:9/a');
    pages.handOver(S, 'page-a');
    const asked = execute(
      deps,
      on('p1', { kind: 'request_user', reason: '请登录测试账号', timeout_ms: 60_000 }),
    );
    await vi.waitFor(() =>
      expect(pages.state(S, 'page-a')?.assist).toMatchObject({ reason: '请登录测试账号' }),
    );
    expect(emulated).toEqual(['page-a']);
    // Answered on that page only.
    expect(assists.answer(S, 'done', 'model')).toBe(false);
    expect(assists.answer(S, 'done', 'page-a')).toBe(true);
    expect((await asked).outcome).toMatchObject({
      outcome: 'completed',
      result: { outcome: 'done' },
    });
    expect(pages.state(S, 'page-a')?.assist).toBeUndefined();
  });
});
