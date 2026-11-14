import { expect, it } from 'vitest';
import {
  addTerminal,
  attachTerminalConsumer,
  receiveTerminalOutput,
  removeTerminal,
  terminalSnapshot,
} from './store';
import type { TerminalApiV1 } from '../../../shared/terminal-api';

it('delivers a delayed initial page before live output and suppresses duplicates', async () => {
  const api = {
    subscribe: async () => ({
      ok: true,
      value: {
        runtime_instance_id: 'runtime',
        session_id: 'session',
        terminal_id: 'test-terminal',
        start_offset: 0,
        next_offset: 0,
        data_base64: '',
        truncated: false,
        end: null,
      },
    }),
  } as unknown as TerminalApiV1;
  const record = await addTerminal(
    {
      terminal_id: 'test-terminal',
      session_id: 'session',
      process_id: 1,
      shell: 'cmd',
      started_at: '2026-10-14T00:00:00Z',
      end: null,
      owner: 'user',
      controller: 'user',
      title: '',
      revision: '1',
    },
    api,
  );
  const output: Uint8Array[] = [];
  attachTerminalConsumer(record, (bytes) => output.push(bytes));
  const batch = {
    runtime_instance_id: 'runtime',
    session_id: 'session',
    terminal_id: 'test-terminal',
    truncated: false,
    end: null,
  };
  receiveTerminalOutput({ ...batch, start_offset: 3, next_offset: 6, data_base64: btoa('two') });
  expect(output).toHaveLength(0);
  receiveTerminalOutput({ ...batch, start_offset: 0, next_offset: 3, data_base64: btoa('one') });
  receiveTerminalOutput({ ...batch, start_offset: 0, next_offset: 3, data_base64: btoa('one') });
  expect(output.map((bytes) => new TextDecoder().decode(bytes)).join('')).toBe('onetwo');
  receiveTerminalOutput({
    ...batch,
    session_id: 'someone-else',
    start_offset: 6,
    next_offset: 9,
    data_base64: btoa('bad'),
  });
  expect(record.offset).toBe(6);
  removeTerminal('test-terminal');
  expect(terminalSnapshot()).toHaveLength(0);
});

const view = (terminal_id: string) => ({
  terminal_id,
  session_id: 'session',
  process_id: 1,
  shell: 'cmd' as const,
  started_at: '2026-10-14T00:00:00Z',
  end: null,
  owner: 'user' as const,
  controller: 'user' as const,
  title: '',
  revision: '1',
});

it('says when the output could not be followed: not subscribed, or out of step', async () => {
  const refused = {
    subscribe: async () => ({
      ok: false,
      error: { code: 'ipc.request_timeout', message: 'Core request timed out' },
    }),
  } as unknown as TerminalApiV1;
  const unsubscribed = await addTerminal(view('unsubscribed'), refused);
  expect(unsubscribed.error).toBe(
    '没有接上这个终端的输出（Aura Core 没有及时回复），屏幕可能不再更新。请关闭这个页签后重新打开终端。',
  );
  removeTerminal('unsubscribed');
  const empty = {
    subscribe: async () => ({
      ok: true,
      value: {
        runtime_instance_id: 'runtime',
        session_id: 'session',
        terminal_id: 'out-of-step',
        start_offset: 0,
        next_offset: 0,
        data_base64: '',
        truncated: false,
        end: null,
      },
    }),
  } as unknown as TerminalApiV1;
  const record = await addTerminal(view('out-of-step'), empty);
  receiveTerminalOutput({
    runtime_instance_id: 'runtime',
    session_id: 'session',
    terminal_id: 'out-of-step',
    truncated: false,
    end: null,
    start_offset: 0,
    next_offset: 5,
    data_base64: btoa('ab'),
  });
  expect(record.error).toBe('终端输出没有对上，屏幕已停止更新。请关闭这个页签后重新打开终端。');
  removeTerminal('out-of-step');
});
