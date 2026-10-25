import { describe, expect, it, vi } from 'vitest';
import {
  registerTerminalOutput,
  terminalOutputNotification as deliver,
} from './terminal-output.js';
import { ProjectionCompatibility } from './projection-compatibility.js';
const guards = new WeakMap<object, ProjectionCompatibility>();
const terminalOutputNotification = (
  ...args: Parameters<typeof deliver> extends [...infer A, unknown] ? A : never
) => {
  const owner = args[0];
  let guard = guards.get(owner);
  if (!guard) {
    guard = new ProjectionCompatibility(vi.fn());
    guards.set(owner, guard);
  }
  return deliver(...args, guard);
};
describe('terminal frame delivery', () => {
  it('publishes only authenticated sensitive metadata and rejects embedded input', () => {
    const owner = {};
    const send = vi.fn();
    registerTerminalOutput(owner, () => ({ isDestroyed: () => false, send }) as never);
    const runtime = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV';
    const pending = { session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV', requests: [] };
    expect(
      terminalOutputNotification(
        owner,
        'terminal.sensitive.v1',
        { runtime_instance_id: runtime, pending },
        runtime,
      ),
    ).toBe(true);
    expect(send).toHaveBeenCalledWith('aura:v1:terminal-sensitive', pending);
    expect(() =>
      terminalOutputNotification(
        owner,
        'terminal.sensitive.v1',
        { runtime_instance_id: runtime, pending: { ...pending, input: 'test' } },
        runtime,
      ),
    ).toThrow();
    expect(() =>
      terminalOutputNotification(
        owner,
        'terminal.sensitive.v1',
        { runtime_instance_id: runtime, pending },
        'other',
      ),
    ).toThrow();
  });
  it('publishes validated ownership changes only from the authenticated Core runtime', () => {
    const owner = {};
    const send = vi.fn();
    registerTerminalOutput(owner, () => ({ isDestroyed: () => false, send }) as never);
    const runtime = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV';
    const params = {
      runtime_instance_id: runtime,
      view: {
        session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        terminal_id: 'term_test',
        shell: 'cmd',
        process_id: 1,
        started_at: '2026-10-14T00:00:00Z',
        end: null,
        owner: 'agent',
        controller: 'user',
        title: 'fixture',
        tool_call_id: null,
        revision: '1',
      },
    };
    expect(terminalOutputNotification(owner, 'terminal.state.v1', params, runtime)).toBe(true);
    expect(send).toHaveBeenCalledWith('aura:v1:terminal-state', params.view);
    expect(() =>
      terminalOutputNotification(owner, 'terminal.state.v1', params, 'other'),
    ).not.toThrow();
    expect(() =>
      terminalOutputNotification(
        owner,
        'terminal.state.v1',
        { ...params, view: { ...params.view, controller: 'model' } },
        runtime,
      ),
    ).not.toThrow();
    expect(send).toHaveBeenCalledTimes(1);
  });
  it('batches output without replacing chunks and rejects another runtime', () => {
    vi.useFakeTimers();
    const owner = {};
    const send = vi.fn();
    registerTerminalOutput(owner, () => ({ isDestroyed: () => false, send }) as never);
    const output = {
      runtime_instance_id: 'runtime',
      session_id: 'session',
      terminal_id: 'term_test',
      start_offset: 0,
      next_offset: 1,
      data_base64: 'YQ==',
      truncated: false,
      end: null,
    };
    // Strict generated validation needs typed identities, so use the shared fixture prefixes.
    output.runtime_instance_id = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV';
    output.session_id = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
    expect(
      terminalOutputNotification(owner, 'terminal.output.v1', output, output.runtime_instance_id),
    ).toBe(true);
    terminalOutputNotification(
      owner,
      'terminal.output.v1',
      { ...output, start_offset: 1, next_offset: 2, data_base64: 'Yg==' },
      output.runtime_instance_id,
    );
    expect(send).not.toHaveBeenCalled();
    vi.advanceTimersByTime(16);
    expect(send).toHaveBeenCalledTimes(1);
    expect(send.mock.calls[0]?.[1]).toHaveLength(2);
    expect(() =>
      terminalOutputNotification(owner, 'terminal.output.v1', output, 'other'),
    ).toThrow();
    vi.useRealTimers();
  });
});
