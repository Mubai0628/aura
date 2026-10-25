import { cleanup, renderHook } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { useToolShortcuts } from './useToolShortcuts';

// Every shortcut the tool menu shows works — not only Ctrl+Shift+G.
afterEach(cleanup);

const press = (init: KeyboardEventInit) => {
  const event = new KeyboardEvent('keydown', { cancelable: true, ...init });
  window.dispatchEvent(event);
  return event;
};

test('each shortcut the tool menu shows opens its tool and releases its listener on unmount', () => {
  const show = vi.fn();
  const hook = renderHook(() => useToolShortcuts(show));
  expect(press({ key: 'G', ctrlKey: true, shiftKey: true }).defaultPrevented).toBe(true);
  press({ key: '`', code: 'Backquote', ctrlKey: true });
  press({ key: 'p', ctrlKey: true });
  press({ key: 't', ctrlKey: true });
  press({ key: 's', ctrlKey: true, altKey: true });
  expect(show.mock.calls.map(([kind]) => kind)).toEqual([
    'review',
    'terminal',
    'files',
    'browser',
    'chat',
  ]);
  hook.unmount();
  press({ key: 'p', ctrlKey: true });
  expect(show).toHaveBeenCalledTimes(5);
});

test('shortcuts respect modal ownership, composition, repeats and other modifiers', () => {
  const show = vi.fn();
  renderHook(() => useToolShortcuts(show));
  const dialog = document.createElement('dialog');
  dialog.setAttribute('open', '');
  document.body.append(dialog);
  press({ key: 'G', ctrlKey: true, shiftKey: true });
  dialog.remove();
  for (const extra of [
    { metaKey: true },
    { repeat: true },
    { isComposing: true },
    { ctrlKey: false },
  ])
    press({ key: 'p', ctrlKey: true, ...extra });
  // Ctrl+Shift+P and Ctrl+Alt+P are not the files shortcut; Ctrl+S alone is not the chat one.
  press({ key: 'P', ctrlKey: true, shiftKey: true });
  press({ key: 'p', ctrlKey: true, altKey: true });
  press({ key: 's', ctrlKey: true });
  expect(show).not.toHaveBeenCalled();
});
