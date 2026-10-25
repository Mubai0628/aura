import { describe, expect, it } from 'vitest';

import { DevToolsWindows, type DevToolsHostWindow, type DevToolsPage } from './devtools-window.js';

// Electron built the DevTools of a
// page shown inside Aura's window and never showed them; each page's DevTools now get a window.

type Listener = () => void;

function fakeWindow(title: string) {
  const closed: Listener[] = [];
  const window = {
    title,
    shown: 0,
    destroyed: false,
    webContents: { id: 900 },
    isDestroyed: () => window.destroyed,
    show: () => {
      window.shown += 1;
    },
    focus: () => undefined,
    close: () => {
      if (window.destroyed) return;
      window.destroyed = true;
      for (const listener of closed) listener();
    },
    once: (_event: 'closed', listener: Listener) => {
      closed.push(listener);
      return window;
    },
  };
  return window;
}

function fakePage(id: number) {
  const destroyed: Listener[] = [];
  const page = {
    id,
    devtoolsIn: undefined as unknown,
    opened: 0,
    closedDevTools: 0,
    gone: false,
    isDestroyed: () => page.gone,
    getTitle: () => 'Example Domain',
    setDevToolsWebContents: (contents: unknown) => {
      page.devtoolsIn = contents;
    },
    openDevTools: () => {
      page.opened += 1;
    },
    closeDevTools: () => {
      page.closedDevTools += 1;
    },
    once: (_event: 'destroyed', listener: Listener) => {
      destroyed.push(listener);
      return page;
    },
    destroy: () => {
      page.gone = true;
      for (const listener of destroyed) listener();
    },
  };
  return page;
}

describe('a page’s DevTools window', () => {
  it('opens in a window of its own, shown, with the page’s name', () => {
    const made: ReturnType<typeof fakeWindow>[] = [];
    const windows = new DevToolsWindows((title) => {
      const window = fakeWindow(title);
      made.push(window);
      return window as unknown as DevToolsHostWindow;
    });
    const page = fakePage(7);
    windows.open(page as unknown as DevToolsPage, undefined);
    expect(made).toHaveLength(1);
    expect(made[0]!.title).toBe('开发者工具 · Example Domain');
    expect(page.devtoolsIn).toBe(made[0]!.webContents);
    expect(page.opened).toBe(1);
    expect(made[0]!.shown).toBe(1);
  });

  it('is brought forward when asked again, not opened twice', () => {
    const made: ReturnType<typeof fakeWindow>[] = [];
    const windows = new DevToolsWindows((title) => {
      const window = fakeWindow(title);
      made.push(window);
      return window as unknown as DevToolsHostWindow;
    });
    const page = fakePage(7);
    windows.open(page as unknown as DevToolsPage, undefined);
    windows.open(page as unknown as DevToolsPage, undefined);
    expect(made).toHaveLength(1);
    expect(made[0]!.shown).toBe(2);
    expect(page.opened).toBe(1);
  });

  it('closes with the page and with Aura’s window, and opens anew after being closed', () => {
    const made: ReturnType<typeof fakeWindow>[] = [];
    const windows = new DevToolsWindows((title) => {
      const window = fakeWindow(title);
      made.push(window);
      return window as unknown as DevToolsHostWindow;
    });
    const page = fakePage(7);
    windows.open(page as unknown as DevToolsPage, undefined);
    made[0]!.close();
    expect(page.closedDevTools).toBe(1);
    windows.open(page as unknown as DevToolsPage, undefined);
    expect(made).toHaveLength(2);
    page.destroy();
    expect(made[1]!.destroyed).toBe(true);

    const owner = fakeWindow('Aura');
    const other = fakePage(8);
    windows.open(other as unknown as DevToolsPage, owner as unknown as DevToolsHostWindow);
    owner.close();
    expect(made[2]!.destroyed).toBe(true);
  });
});
