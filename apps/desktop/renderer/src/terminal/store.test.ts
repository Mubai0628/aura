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
