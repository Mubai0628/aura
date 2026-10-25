import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { TerminalRecord } from './store';
import { disposeTerminalEngine, terminalEngine } from './xterm-runtime';

// Ctrl+V once showed "无法读取剪贴板" and pasted nothing: the terminal
// asked navigator.clipboard for permissions Main refuses to every page; copy failed the same way.
// Both now leave the work to the browser's own clipboard events, which xterm handles.

const state = vi.hoisted(() => ({
  keys: undefined as ((event: KeyboardEvent) => boolean) | undefined,
  selection: '',
  textarea: undefined as HTMLTextAreaElement | undefined,
  write: vi.fn(async () => ({ ok: true })),
}));
vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    textarea = document.createElement('textarea');
    constructor() {
      state.textarea = this.textarea;
    }
    loadAddon() {}
    open() {}
    onData() {
      return { dispose() {} };
    }
    attachCustomKeyEventHandler(handler: (event: KeyboardEvent) => boolean) {
      state.keys = handler;
    }
    hasSelection() {
      return state.selection !== '';
    }
    getSelection() {
      return state.selection;
    }
    write(_data: Uint8Array, done?: () => void) {
      done?.();
    }
    dispose() {}
  },
}));
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class {} }));
vi.mock('./store', () => ({
  attachTerminalConsumer() {},
  configureTerminalRenderer() {},
  terminalApi: () => ({ write: state.write }),
  terminalFailure: vi.fn(),
}));
vi.mock('./csp-styles', () => ({ adoptTerminalStyles: () => () => {} }));

const clipboard = { readText: vi.fn(), writeText: vi.fn() };
const record = {
  view: { session_id: 's', terminal_id: 'clipboard-test', controller: 'user' },
} as unknown as TerminalRecord;
const press = (key: string) => {
  const event = new KeyboardEvent('keydown', { key, ctrlKey: true, cancelable: true });
  return { handled: state.keys!(event), event };
};
beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal('requestAnimationFrame', () => 1);
  vi.stubGlobal('cancelAnimationFrame', () => {});
  vi.stubGlobal('navigator', { clipboard });
  state.selection = '';
  state.write.mockClear();
  clipboard.readText.mockClear();
  clipboard.writeText.mockClear();
  terminalEngine(record);
});
afterEach(() => {
  disposeTerminalEngine('clipboard-test');
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it('leaves Ctrl+V to the browser paste, without reading the clipboard', () => {
  const { handled, event } = press('v');
  expect(handled).toBe(false);
  expect(event.defaultPrevented).toBe(false);
  expect(clipboard.readText).not.toHaveBeenCalled();
  expect(state.write).not.toHaveBeenCalled();
});

it('copies a selection with Ctrl+C through the browser copy and does not interrupt', () => {
  state.selection = 'PASTE-OK-4242';
  const { handled, event } = press('c');
  expect(handled).toBe(false);
  expect(event.defaultPrevented).toBe(false);
  const textarea = state.textarea!;
  expect(textarea.value).toBe('PASTE-OK-4242');
  expect([textarea.selectionStart, textarea.selectionEnd]).toEqual([0, 13]);
  vi.runAllTimers();
  expect(textarea.value).toBe('');
  expect(clipboard.writeText).not.toHaveBeenCalled();
  expect(state.write).not.toHaveBeenCalled();
});

it('interrupts with Ctrl+C when nothing is selected', () => {
  const { handled, event } = press('c');
  expect(handled).toBe(false);
  expect(event.defaultPrevented).toBe(true);
  expect(state.write).toHaveBeenCalledWith(expect.objectContaining({ input: '', interrupt: true }));
});
