// @vitest-environment node
import { describe, expect, it } from 'vitest';

import { execute } from './actions.js';
import { command, S, setup } from './executor-fakes.js';
import { setup as pageSetup, POLICY } from './page-test-support.js';

describe("carrying out Core's command on the model page", () => {
  it('opens a newly approved address after its released old address is no longer approved', async () => {
    const { pages, view } = pageSetup(1);
    pages.setPolicy(POLICY);
    const { deps } = setup({ pages });
    const old = command({ kind: 'open', url: 'https://old.example/' });
    old.access = { read_only: true, approved_hosts: ['old.example'], file_root: null };
    expect((await execute(deps, old)).outcome.outcome).toBe('completed');
    view(0).webContents.fire('did-navigate', {}, 'https://old.example/');
    pages.open(S, 'new');
    const next = command({ kind: 'open', url: 'https://new.example/' });
    next.access = { read_only: true, approved_hosts: ['new.example'], file_root: null };
    expect((await execute(deps, next)).outcome.outcome).toBe('completed');
    expect(pages.contents(S, 'model')?.getURL()).toBe('https://new.example/');
  });

  it('opens the page and answers with its snapshot', async () => {
    const { deps, pages } = setup();
    const result = await execute(deps, command({ kind: 'open', url: 'http://127.0.0.1:9/game' }));
    expect(pages.contents(S, 'model')?.getURL()).toBe('http://127.0.0.1:9/game');
    expect(result.page_url).toBe('http://127.0.0.1:9/game');
    expect(result.outcome).toMatchObject({
      outcome: 'completed',
      result: {
        url: 'http://127.0.0.1:9/game',
        title: 'Game',
        snapshot: expect.stringContaining('button "Start" [ref=e1]'),
      },
    });
    expect(pages.state(S, 'model')?.agentActive).toBe(false);
  });

  it('acts on nothing before a page is open', async () => {
    const { deps } = setup();
    const result = await execute(deps, command({ kind: 'snapshot', max_chars: 12_000 }));
    expect(result.outcome).toMatchObject({
      outcome: 'refused',
      message: expect.stringContaining('browser.open'),
    });
  });

  it('clicks where the element is, in the view the page is drawn at', async () => {
    const { deps, tools } = setup();
    await execute(deps, command({ kind: 'open', url: 'http://127.0.0.1:9/game' }));
    const result = await execute(
      deps,
      command(
        { kind: 'click', element: 'e1', x: null, y: null, button: 'left', double: false },
        '127.0.0.1',
      ),
    );
    expect(result.outcome.outcome).toBe('completed');
    const pressed = tools.sent.find(
      ([method, params]) => method === 'Input.dispatchMouseEvent' && params.type === 'mousePressed',
    );
    // The element's centre is (120, 210) on the page; the page is drawn at half size.
    expect(pressed?.[1]).toMatchObject({ x: 60, y: 105, button: 'left', clickCount: 1 });
  });

  it('refuses input on a page that has moved since Core approved it, or that the user has taken over', async () => {
    const { deps, pages, tools } = setup();
    await execute(deps, command({ kind: 'open', url: 'http://127.0.0.1:9/game' }));
    const click = (expected: string) =>
      execute(
        deps,
        command(
          { kind: 'click', element: 'e1', x: null, y: null, button: 'left', double: false },
          expected,
        ),
      );
    expect((await click('other.example')).outcome).toMatchObject({
      outcome: 'refused',
      message: expect.stringContaining('other.example'),
    });
    pages.setUserControl(S, 'model', true);
    expect((await click('127.0.0.1')).outcome).toMatchObject({
      outcome: 'refused',
      message: expect.stringContaining('交还 Aura'),
    });
    expect(tools.methods()).not.toContain('Input.dispatchMouseEvent');
    // Reading the page is still fine while the user has it.
    expect(
      (await execute(deps, command({ kind: 'snapshot', max_chars: 12_000 }))).outcome.outcome,
    ).toBe('completed');
  });

  it('never types into a password field', async () => {
    const { deps, tools } = setup();
    await execute(deps, command({ kind: 'open', url: 'http://127.0.0.1:9/game' }));
    await execute(deps, command({ kind: 'snapshot', max_chars: 12_000 }));
    const result = await execute(
      deps,
      command(
        { kind: 'type', element: 'e2', text: 'hunter2', clear: false, submit: false },
        '127.0.0.1',
      ),
    );
    expect(result.outcome).toMatchObject({
      outcome: 'refused',
      message: expect.stringContaining('敏感字段'),
    });
    expect(tools.methods()).not.toContain('Input.insertText');
  });

  it('presses keys as DevTools key events and refuses a key it cannot press', async () => {
    const { deps, tools } = setup();
    await execute(deps, command({ kind: 'open', url: 'http://127.0.0.1:9/game' }));
    await execute(
      deps,
      command({ kind: 'press', key: 'ArrowLeft', repeat: 2, hold_ms: 0 }, '127.0.0.1'),
    );
    const keys = tools.sent
      .filter(([method]) => method === 'Input.dispatchKeyEvent')
      .map(([, p]) => `${String(p.type)}:${String(p.key)}`);
    expect(keys).toEqual([
      'rawKeyDown:ArrowLeft',
      'keyUp:ArrowLeft',
      'rawKeyDown:ArrowLeft',
      'keyUp:ArrowLeft',
    ]);
    const wrong = await execute(
      deps,
      command({ kind: 'press', key: 'Hyper+x', repeat: 1, hold_ms: 0 }, '127.0.0.1'),
    );
    expect(wrong.outcome.outcome).toBe('refused');
  });

  it('stops while the browser is switched off', async () => {
    const { deps, pages } = setup();
    pages.setPolicy({ enabled: false, allowed: [], blocked: [] });
    const result = await execute(deps, command({ kind: 'open', url: 'http://127.0.0.1:9/game' }));
    expect(result.outcome).toMatchObject({
      outcome: 'refused',
      message: expect.stringContaining('已在设置中关闭'),
    });
  });
});
