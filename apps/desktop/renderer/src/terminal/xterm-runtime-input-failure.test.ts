import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { TerminalRecord } from './store';
import { disposeTerminalEngine, terminalEngine } from './xterm-runtime';

// Typed input that did not reach the terminal says so until the next input gets through; a failure
// of anything else is left as it is.

const state = vi.hoisted(() => ({
  data: undefined as ((value: string) => void) | undefined,
  write: vi.fn(),
  failure: vi.fn(),
  cleared: vi.fn(),
}));
vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    textarea = document.createElement('textarea');
    loadAddon() {}
    open() {}
    onData(handler: (value: string) => void) {
      state.data = handler;
      return { dispose() {} };
    }
    attachCustomKeyEventHandler() {}
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
  configureTerminalText() {},
  terminalApi: () => ({ write: state.write }),
  terminalFailure: state.failure,
  clearTerminalFailure: state.cleared,
}));
vi.mock('./csp-styles', () => ({ adoptTerminalStyles: () => () => {} }));

const record = {
  view: { session_id: 's', terminal_id: 'input-failure-test', controller: 'user' },
} as unknown as TerminalRecord;

beforeEach(() => {
  vi.stubGlobal('requestAnimationFrame', () => 1);
  vi.stubGlobal('cancelAnimationFrame', () => {});
  state.write.mockReset();
  state.failure.mockReset();
  state.cleared.mockReset();
  terminalEngine(record);
});
afterEach(() => {
  disposeTerminalEngine('input-failure-test');
  vi.unstubAllGlobals();
});

it('says typed input was lost, and clears that once input gets through again', async () => {
  const lost =
    '这次输入没有送到终端（与 Aura Core 的连接断开了），终端里可能缺了这段输入。请看终端当前内容再决定是否重新输入。';
  state.write.mockResolvedValueOnce({
    ok: false,
    error: { code: 'ipc.transport_lost', message: 'Transport is closed' },
  });
  state.data!('ls');
  await vi.waitFor(() => expect(state.failure).toHaveBeenCalledWith(record, lost));
  state.write.mockResolvedValueOnce({ ok: true, value: {} });
  state.data!('\r');
  await vi.waitFor(() => expect(state.cleared).toHaveBeenCalledWith(record, lost));
  // Nothing to clear once it was cleared.
  state.write.mockResolvedValueOnce({ ok: true, value: {} });
  state.data!('x');
  await vi.waitFor(() => expect(state.write).toHaveBeenCalledTimes(3));
  expect(state.cleared).toHaveBeenCalledTimes(1);
});
