import type { BrowserWindow } from 'electron';
import { expect, test } from 'vitest';
import { liveContents } from './live-window.js';

function fakeWindow(destroyed: boolean) {
  const contents = { id: 7 };
  return {
    isDestroyed: () => destroyed,
    get webContents() {
      if (destroyed) throw new TypeError('Object has been destroyed');
      return contents;
    },
  } as unknown as BrowserWindow;
}

test('a closed window has no contents instead of throwing', () => {
  // What Main read before: a terminal output timer threw this after the window closed, and the
  // error box kept Aura from quitting.
  expect(() => fakeWindow(true)?.webContents).toThrow('Object has been destroyed');
  expect(liveContents(fakeWindow(true))).toBeUndefined();
  expect(liveContents(undefined)).toBeUndefined();
  expect(liveContents(fakeWindow(false))).toEqual({ id: 7 });
});
