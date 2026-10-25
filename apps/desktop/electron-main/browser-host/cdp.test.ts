// @vitest-environment node
import { describe, expect, it } from 'vitest';

import { PageDebugger, type DevTools } from './cdp.js';

class FakeDevTools implements DevTools {
  attached = false;
  sent: [string, object | undefined][] = [];
  listeners = new Map<string, ((...args: never[]) => void)[]>();
  attach() {
    this.attached = true;
  }
  isAttached() {
    return this.attached;
  }
  sendCommand(method: string, params?: object) {
    this.sent.push([method, params]);
    return Promise.resolve({});
  }
  on(event: string, listener: (...args: never[]) => void) {
    this.listeners.set(event, [...(this.listeners.get(event) ?? []), listener]);
    return this;
  }
  emit(method: string, params: object) {
    for (const listener of this.listeners.get('message') ?? [])
      (listener as (...a: unknown[]) => void)({}, method, params);
  }
}

describe("a page's DevTools connection", () => {
  it('attaches once and enables what it reads', async () => {
    const devtools = new FakeDevTools();
    const page = new PageDebugger(devtools);
    await Promise.all([page.ready(), page.ready()]);
    expect(devtools.attached).toBe(true);
    expect(devtools.sent.map(([method]) => method)).toEqual([
      'Page.enable',
      'Runtime.enable',
      'DOM.enable',
      'Accessibility.enable',
      'Network.enable',
      'Log.enable',
      'Overlay.enable',
    ]);
  });

  it('records console messages, uncaught errors and failed loads, and reports each once', () => {
    const devtools = new FakeDevTools();
    const page = new PageDebugger(devtools);
    devtools.emit('Runtime.consoleAPICalled', {
      type: 'log',
      args: [
        { type: 'string', value: 'score' },
        { type: 'number', value: 3 },
      ],
    });
    devtools.emit('Runtime.consoleAPICalled', {
      type: 'error',
      args: [{ type: 'string', value: 'boom' }],
      stackTrace: { callFrames: [{ url: 'http://127.0.0.1:9/game.js', lineNumber: 11 }] },
    });
    devtools.emit('Runtime.exceptionThrown', {
      exceptionDetails: {
        text: 'Uncaught',
        url: 'http://127.0.0.1:9/game.js',
        lineNumber: 4,
        exception: { description: 'ReferenceError: board is not defined' },
      },
    });
    devtools.emit('Network.requestWillBeSent', {
      requestId: '1',
      request: { url: 'http://127.0.0.1:9/a.png' },
    });
    devtools.emit('Network.loadingFailed', {
      requestId: '1',
      errorText: 'net::ERR_BLOCKED_BY_CLIENT',
    });
    devtools.emit('Network.responseReceived', {
      requestId: '2',
      response: { status: 404, url: 'http://127.0.0.1:9/missing.js' },
    });
    devtools.emit('Network.loadingFailed', { requestId: '3', errorText: 'net::ERR_ABORTED' });
    expect(page.takeNew()).toEqual([
      { level: 'log', text: 'score 3', source: null },
      { level: 'error', text: 'boom', source: 'http://127.0.0.1:9/game.js:12' },
      {
        level: 'exception',
        text: 'ReferenceError: board is not defined',
        source: 'http://127.0.0.1:9/game.js:5',
      },
      {
        level: 'failed_load',
        text: 'net::ERR_BLOCKED_BY_CLIENT（浏览器设置不允许） http://127.0.0.1:9/a.png',
        source: null,
      },
      { level: 'failed_load', text: 'HTTP 404 http://127.0.0.1:9/missing.js', source: null },
    ]);
    expect(page.takeNew()).toEqual([]);
    expect(page.all('error', 50).entries.map((entry) => entry.level)).toEqual([
      'error',
      'exception',
      'failed_load',
      'failed_load',
    ]);
    expect(page.all('all', 2)).toMatchObject({ truncated: true });
  });

  it('answers dialogs so a page never waits on one, and says how', () => {
    const devtools = new FakeDevTools();
    const page = new PageDebugger(devtools);
    devtools.emit('Page.javascriptDialogOpening', { type: 'alert', message: 'Game over' });
    devtools.emit('Page.javascriptDialogOpening', { type: 'confirm', message: 'Play again?' });
    expect(devtools.sent).toEqual([
      ['Page.handleJavaScriptDialog', { accept: true }],
      ['Page.handleJavaScriptDialog', { accept: false }],
    ]);
    expect(page.takeDialogs()).toEqual([
      'alert: Game over（Aura 已确认）',
      'confirm: Play again?（Aura 已取消）',
    ]);
    expect(page.takeDialogs()).toEqual([]);
  });

  it('forgets the refs of a page it navigated away from', () => {
    const devtools = new FakeDevTools();
    const page = new PageDebugger(devtools);
    page.refs.refFor(42);
    devtools.emit('Page.frameNavigated', { frame: { id: 'child', parentId: 'main' } });
    expect(page.refs.node('e1')).toBe(42);
    devtools.emit('Page.frameNavigated', { frame: { id: 'main' } });
    expect(page.refs.node('e1')).toBeUndefined();
  });
});
