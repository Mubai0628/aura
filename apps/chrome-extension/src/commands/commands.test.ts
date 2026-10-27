import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import type { DebuggerApi, Tab } from '../chrome-api.js';
import { Pages } from '../page/pages.js';
import type { Target } from '../page/inspect.js';
import { CommandError, type ChromeCommand, type PageReport } from '../protocol.js';
import type { CommandContext } from './context.js';
import { parseCommand, run } from './index.js';

type Handler = (params: Record<string, unknown> | undefined) => unknown;

class FakeDebugger implements DebuggerApi {
  readonly sent: { method: string; params: Record<string, unknown> | undefined }[] = [];
  readonly handlers: Record<string, Handler> = {};
  refuseAttach: string | null = null;
  async attach(): Promise<void> {
    if (this.refuseAttach !== null) throw new Error(this.refuseAttach);
  }
  async detach(): Promise<void> {}
  async sendCommand(_target: { tabId: number }, method: string, params?: object): Promise<unknown> {
    const given = params as Record<string, unknown> | undefined;
    this.sent.push({ method, params: given });
    return this.handlers[method]?.(given) ?? {};
  }
  onEvent = { addListener: () => undefined, removeListener: () => undefined };
  onDetach = { addListener: () => undefined, removeListener: () => undefined };
  inputs(): string[] {
    return this.sent.map((call) => call.method).filter((method) => method.startsWith('Input.'));
  }
}

const SHOP: Tab = {
  id: 7,
  windowId: 1,
  url: 'https://shop.example/cart',
  title: 'Cart',
  active: true,
  groupId: -1,
  status: 'complete',
};

/** A page with one button, e1 once it is read, at (10,20)-(110,60). */
function page(fake: FakeDebugger, target: Partial<Target> = {}): void {
  fake.handlers['Accessibility.getFullAXTree'] = () => ({
    nodes: [
      { nodeId: '1', role: { value: 'RootWebArea' }, childIds: ['2'] },
      {
        nodeId: '2',
        parentId: '1',
        role: { value: 'button' },
        name: { value: 'Checkout' },
        backendDOMNodeId: 11,
      },
    ],
  });
  fake.handlers['DOM.getContentQuads'] = () => ({ quads: [[10, 20, 110, 20, 110, 60, 10, 60]] });
  fake.handlers['DOM.resolveNode'] = () => ({ object: { objectId: 'o1' } });
  fake.handlers['Runtime.callFunctionOn'] = () => ({
    result: {
      value: {
        tag: 'button',
        attributes: {},
        text: 'Checkout',
        paymentForm: false,
        captcha: false,
        fileChooser: false,
        ...target,
      },
    },
  });
}

function context(fake: FakeDebugger, tab: Tab = SHOP): CommandContext {
  return {
    tabs: {
      get: async () => tab,
      create: async () => tab,
      remove: async () => undefined,
    },
    pages: new Pages(fake, { set: () => 0, clear: () => undefined }),
    group: async () => undefined,
    assist: async () => 'done',
    signal: new AbortController().signal,
    sleep: async () => undefined,
    now: () => 0,
  };
}

async function failure(work: Promise<unknown>): Promise<CommandError> {
  const error = await work.then(
    () => null,
    (thrown: unknown) => thrown,
  );
  expect(error).toBeInstanceOf(CommandError);
  return error as CommandError;
}

const CLICK: ChromeCommand = {
  op: 'click',
  tab_id: 7,
  element: 'e1',
  x: null,
  y: null,
  button: 'left',
  double: false,
};

describe('commands from Core', () => {
  it('accept every shape Core sends, and nothing else', () => {
    const fixture = JSON.parse(
      readFileSync(
        new URL('../../../../fixtures/chrome/protocol.v1.json', import.meta.url),
        'utf8',
      ),
    ) as { commands: unknown[] };
    for (const command of fixture.commands) expect(parseCommand(command)).toEqual(command);
    for (const wrong of [
      { op: 'snapshot', tab_id: 7 },
      { op: 'snapshot', tab_id: 7, max_chars: 10, extra: 1 },
      { op: 'scroll', tab_id: 7, direction: 'sideways', element: null, amount: 1 },
      { op: 'evaluate', tab_id: 7, expression: '1' },
      { op: 'constructor' },
      null,
    ])
      expect(() => parseCommand(wrong), JSON.stringify(wrong)).toThrow(CommandError);
  });

  it('read the tab with refs for its actionable elements', async () => {
    const fake = new FakeDebugger();
    page(fake);
    const report = (await run(
      context(fake),
      { op: 'snapshot', tab_id: 7, max_chars: 12_000 },
      'https://shop.example',
    )) as PageReport;
    expect(report.url).toBe(SHOP.url);
    expect(report.snapshot).toContain('button "Checkout" [ref=e1]');
  });

  it('click the middle of the element after checking it is not the user’s alone', async () => {
    const fake = new FakeDebugger();
    page(fake);
    const ctx = context(fake);
    await run(ctx, { op: 'snapshot', tab_id: 7, max_chars: 12_000 }, undefined);
    await run(ctx, CLICK, 'https://shop.example');
    const pressed = fake.sent.find(
      (call) => call.method === 'Input.dispatchMouseEvent' && call.params?.type === 'mousePressed',
    );
    expect(pressed?.params).toMatchObject({ x: 60, y: 40, button: 'left', clickCount: 1 });
  });

  it('change nothing in a tab that left the site Core authorized', async () => {
    const fake = new FakeDebugger();
    page(fake);
    const moved = { ...SHOP, url: 'https://pay.example/checkout' };
    const error = await failure(run(context(fake, moved), CLICK, 'https://shop.example'));
    expect(error.code).toBe('chrome.refused');
    expect(error.message).toContain('https://pay.example');
    expect(fake.sent).toEqual([]);
  });

  it('refuse, as they run, typing into a password field and clicking into a CAPTCHA', async () => {
    const fake = new FakeDebugger();
    page(fake, { tag: 'input', attributes: { type: 'password' } });
    const ctx = context(fake);
    await run(ctx, { op: 'snapshot', tab_id: 7, max_chars: 12_000 }, undefined);
    const typed = await failure(
      run(
        ctx,
        { op: 'type', tab_id: 7, element: 'e1', text: 'hunter2', clear: false, submit: false },
        'https://shop.example',
      ),
    );
    expect(typed.code).toBe('chrome.refused');
    page(fake, { captcha: true });
    const clicked = await failure(run(ctx, CLICK, 'https://shop.example'));
    expect(clicked.message).toContain('CAPTCHA');
    expect(fake.inputs()).toEqual([]);
  });

  it('report a tab Chrome will not let Aura debug as a refusal that changed nothing', async () => {
    const fake = new FakeDebugger();
    fake.refuseAttach = 'Another debugger is already attached to the tab';
    const error = await failure(
      run(context(fake), { op: 'snapshot', tab_id: 7, max_chars: 12_000 }, undefined),
    );
    expect(error.code).toBe('chrome.refused');
    expect(error.message).toContain('Another debugger');
  });

  it('inspect a click by position at the element under it', async () => {
    const fake = new FakeDebugger();
    page(fake, { text: 'Place order' });
    fake.handlers['DOM.getNodeForLocation'] = () => ({ backendNodeId: 11 });
    const inspection = await run(
      context(fake),
      { op: 'inspect', tab_id: 7, element: null, x: 60, y: 40, intent: 'click' },
      undefined,
    );
    expect(inspection).toEqual({ refused: null, sensitive: true, origin: 'https://shop.example' });
    expect(
      fake.sent.find((call) => call.method === 'DOM.getNodeForLocation')?.params,
    ).toMatchObject({ x: 60, y: 40 });
  });
});
