// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import type { PanelRequest, PanelState } from '../panel-messages.js';
import { render, shareable, type Form, type ViewModel } from './view.js';

const connected: PanelState = {
  connection: { kind: 'connected' },
  sessions: [
    { session_id: 'ses_one', title: 'Docs', workspace: 'Aura' },
    { session_id: 'ses_two', title: 'Game', workspace: 'Play' },
  ],
  shares: [],
  assist: null,
  share_refused: null,
};

const TAB = { tab_id: 7, window_id: 1, url: 'https://shop.example/cart', title: 'Cart' };

function draw(
  state: PanelState,
  tab = TAB,
  form: Form = { code: '', session: '', scope: 'session' },
) {
  const root = document.createElement('main');
  const sent: PanelRequest[] = [];
  const edits: Partial<Form>[] = [];
  const model: ViewModel = { state, tab, form, now: 0 };
  render(root, model, {
    send: (request) => sent.push(request),
    edit: (change) => edits.push(change),
  });
  const press = (label: string) => {
    const found = Array.from(root.querySelectorAll('button')).find(
      (node) => node.textContent === label,
    );
    if (found === undefined) throw new Error(`no button ${label}`);
    found.click();
  };
  return { root, sent, edits, press };
}

describe('the side panel', () => {
  it('asks for the pairing code until paired, and says why a code was refused', () => {
    const { root, sent, press } = draw({
      ...connected,
      connection: { kind: 'pairing_required', refused: 'code_expired' },
    });
    const input = root.querySelector('input.code') as HTMLInputElement;
    input.value = '12345678';
    press('配对');
    expect(sent).toEqual([{ type: 'pair', code: '12345678' }]);
    expect(root.textContent).toContain('配对码已过期');
    expect(root.textContent).not.toContain('共享给 Aura');
  });

  it('shares the current tab with the chosen conversation and scope', () => {
    const { root, sent, press } = draw(connected, TAB, {
      code: '',
      session: 'ses_two',
      scope: 'turn',
    });
    expect((root.querySelector('select.sessions') as HTMLSelectElement).value).toBe('ses_two');
    press('共享给 Aura');
    expect(sent).toEqual([
      {
        type: 'share',
        tab_id: 7,
        window_id: 1,
        url: TAB.url,
        title: 'Cart',
        session_id: 'ses_two',
        scope: 'turn',
      },
    ]);
  });

  it('answers a new site of a shared tab, and stops everything', () => {
    const awaiting: PanelState = {
      ...connected,
      shares: [
        {
          tab_id: 7,
          session_id: 'ses_one',
          session_title: 'Docs',
          origin: 'https://pay.example',
          state: 'awaiting_origin',
          scope: 'session',
        },
      ],
    };
    const { root, sent, press } = draw(awaiting);
    // The shared current tab is the list's; no card above repeats it.
    expect(root.querySelector('section.current')).toBeNull();
    expect(root.textContent).not.toContain('已共享给');
    expect(root.querySelector('li.share')?.textContent).toContain('Docs');
    expect(root.textContent).toContain('等待确认新网站');
    press('允许');
    press('全部停止');
    expect(sent).toEqual([
      { type: 'answer_origin', tab_id: 7, origin: 'https://pay.example', allow: true },
      { type: 'stop' },
    ]);
  });

  it('stops one shared page from the list, and every page with 全部停止', () => {
    const share = {
      session_id: 'ses_one',
      session_title: 'Docs',
      origin: 'https://docs.example',
      scope: 'session',
    } as const;
    const { root, sent, press } = draw(
      {
        ...connected,
        shares: [
          { ...share, tab_id: 7, state: 'active' },
          { ...share, tab_id: 9, origin: 'https://wiki.example', state: 'with_user' },
        ],
      },
      { ...TAB, tab_id: 3 },
    );
    const items = Array.from(root.querySelectorAll('li.share'));
    expect(items).toHaveLength(2);
    const stop = items[1]?.querySelector('button');
    expect(stop?.textContent).toBe('停止共享');
    stop?.click();
    press('全部停止');
    expect(sent).toEqual([{ type: 'unshare', tab_id: 9 }, { type: 'stop' }]);
    expect(
      Array.from(root.querySelectorAll('button')).some((node) => node.textContent === '停止'),
    ).toBe(false);
  });

  it('shows a tab handed to the user with the time left and the user’s two answers', () => {
    const { root, sent, press } = draw({
      ...connected,
      assist: { tab_id: 7, reason: '请在页面上登录', deadline: 125_000 },
    });
    expect(root.textContent).toContain('请在页面上登录');
    expect(root.textContent).toContain('2:05');
    press('完成');
    press('取消');
    expect(sent).toEqual([
      { type: 'assist', outcome: 'done' },
      { type: 'assist', outcome: 'declined' },
    ]);
  });

  it('offers only ordinary web pages for sharing', () => {
    expect(shareable('https://shop.example/')).toBe(true);
    expect(shareable('http://localhost:5173/')).toBe(true);
    for (const url of ['chrome://settings', 'https://chromewebstore.google.com/x', 'file:///C:/x'])
      expect(shareable(url), url).toBe(false);
    const { root } = draw(connected, { ...TAB, url: 'chrome://settings' });
    expect(root.textContent).toContain('此页面不能共享');
  });
});
