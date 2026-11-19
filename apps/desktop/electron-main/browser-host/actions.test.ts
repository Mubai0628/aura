// @vitest-environment node
import { describe, expect, it, vi } from 'vitest';

import { execute } from './actions.js';
import { command, S, setup } from './executor-fakes.js';
import { setup as pageSetup, POLICY } from './page-test-support.js';

describe("carrying out Core's command on the model page", () => {
  it('carries out a debugging command only on the observed document and settings revision', async () => {
    const { pages, view } = pageSetup();
    pages.setPolicy({ ...POLICY, revision: 7, debugging: false });
    const { deps } = setup({ pages });
    const opened = await execute(deps, command({ kind: 'open', url: 'https://old.example/' }));
    expect(opened.document_id).toBeTruthy();
    const read = command(
      {
        kind: 'network',
        failed_only: true,
        max_entries: 10,
        binding: {
          document_id: opened.document_id!,
          settings_revision: 7,
          origin: 'https://old.example',
        },
      },
      'old.example',
    );
    expect((await execute(deps, read)).outcome.outcome).toBe('completed');
    view(0).webContents.fire('did-navigate', {}, 'https://old.example/');
    expect((await execute(deps, read)).outcome.outcome).toBe('refused');
  });

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

  it('links a late blocked download to the click without reporting a saved file', async () => {
    const { deps, pages } = setup();
    await execute(deps, command({ kind: 'open', url: 'http://127.0.0.1:9/game' }));
    const click = command(
      { kind: 'click', element: 'e1', button: 'left', double: false },
      '127.0.0.1',
    );
    const first = await execute(deps, click);
    expect(first.outcome).toMatchObject({
      outcome: 'completed',
      result: { download: { status: 'not_observed', click_id: click.command_id } },
    });

    const now = Date.now() + 13_000;
    const clock = vi.spyOn(Date, 'now').mockReturnValue(now);
    try {
      pages.notice(pages.contents(S, 'model')!.id, 'download', '已阻止', 'paper.pdf');
      const snapshot = await execute(deps, command({ kind: 'snapshot', max_chars: 12_000 }));
      expect(snapshot.outcome).toMatchObject({
        outcome: 'completed',
        result: {
          download: {
            status: 'blocked_by_host',
            click_id: click.command_id,
            filename: 'paper.pdf',
          },
        },
      });
    } finally {
      clock.mockRestore();
    }
  });

  it('returns access_required on an HTTP 200 login screen with a visible gate only', async () => {
    const { deps, tools } = setup();
    const url = 'http://127.0.0.1:9/gate';
    await execute(deps, command({ kind: 'open', url }));
    tools.emit('Network.requestWillBeSent', {
      requestId: 'document',
      type: 'Document',
      request: { url },
    });
    tools.emit('Network.responseReceived', {
      requestId: 'document',
      response: { status: 200, mimeType: 'text/html' },
    });
    let gate = false;
    tools.answers.set('Runtime.evaluate', ({ expression }) => ({
      result: { value: String(expression).includes('gate.some(visible)') ? gate : undefined },
    }));
    const click = command(
      { kind: 'click', element: 'e1', button: 'left', double: false },
      '127.0.0.1',
    );
    expect((await execute(deps, click)).outcome).toMatchObject({
      result: { download: { status: 'not_observed', click_id: click.command_id } },
    });
    gate = true;
    expect(
      (await execute(deps, command({ kind: 'snapshot', max_chars: 12_000 }))).outcome,
    ).toMatchObject({
      result: { download: { status: 'access_required', click_id: click.command_id } },
    });
  });

  it('does not guess which nearby click caused a blocked download', async () => {
    const { deps, pages } = setup();
    await execute(deps, command({ kind: 'open', url: 'http://127.0.0.1:9/game' }));
    const click = command(
      { kind: 'click', element: 'e1', button: 'left', double: false },
      '127.0.0.1',
    );
    await execute(deps, click);
    await execute(deps, { ...click, command_id: 'bcm-2' });
    pages.notice(pages.contents(S, 'model')!.id, 'download', '已阻止', 'paper.pdf');
    const snapshot = await execute(deps, command({ kind: 'snapshot', max_chars: 12_000 }));
    expect(snapshot.outcome).toMatchObject({
      outcome: 'completed',
      result: { download: { status: 'blocked_by_host', click_id: null, filename: 'paper.pdf' } },
    });
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

  it('tells an exception in the page in a sentence that names the step, with its first line', async () => {
    const { deps, tools } = setup();
    await execute(deps, command({ kind: 'open', url: 'http://127.0.0.1:9/game' }));
    tools.answers.set('Accessibility.getFullAXTree', () => {
      throw new Error('Execution context was destroyed.\n    at Page.snapshot');
    });
    const read = await execute(deps, command({ kind: 'snapshot', max_chars: 12_000 }));
    expect(read.outcome).toEqual({
      outcome: 'failed',
      message: '页面执行这一步（snapshot）时出错：Execution context was destroyed。',
      may_have_acted: false,
    });
    tools.answers.delete('Accessibility.getFullAXTree');
    tools.answers.set('Input.dispatchMouseEvent', () => {
      throw new Error('Target closed');
    });
    const click = await execute(
      deps,
      command(
        { kind: 'click', element: 'e1', x: null, y: null, button: 'left', double: false },
        '127.0.0.1',
      ),
    );
    expect(click.outcome).toEqual({
      outcome: 'failed',
      message: '页面执行这一步（click）时出错：Target closed。',
      may_have_acted: true,
    });
  });
});
