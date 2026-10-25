// @vitest-environment node
import { EventEmitter } from 'node:events';
import type { BrowserWindow } from 'electron';
import { expect, test, vi } from 'vitest';

const hooks = vi.hoisted(() => ({
  created: undefined as undefined | ((event: unknown, window: EventEmitter) => void),
}));
vi.mock('electron', () => ({
  app: {
    once: vi.fn(),
    on: vi.fn((event: string, listener: typeof hooks.created) => {
      if (event === 'browser-window-created') hooks.created = listener;
    }),
  },
}));
vi.mock('./workspace-media.js', () => ({ WorkspaceMedia: class {} }));
import { createMediaHosts } from './embedded-hosts.js';

test('closing the visible owner disposes media; closing a decoder or replaced renderer does not', () => {
  const first = new EventEmitter(),
    replacement = new EventEmitter(),
    decoder = new EventEmitter();
  let owner = first;
  const { mediaHost } = createMediaHosts(() => owner as unknown as BrowserWindow);
  const dispose = vi.spyOn(mediaHost, 'dispose');
  for (const window of [first, replacement, decoder]) hooks.created!({}, window);
  decoder.emit('closed');
  expect(dispose).not.toHaveBeenCalled();
  owner = replacement;
  first.emit('closed');
  expect(dispose).not.toHaveBeenCalled();
  replacement.emit('closed');
  expect(dispose).toHaveBeenCalledTimes(1);
});
