import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { ShellExecutionView } from '@aura/contracts/generated';
import { ShellExecutionDetails } from './ShellExecutionDetails';

const readShellExecution = vi.fn(),
  stopShellExecution = vi.fn();
const call = 'tcl_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const view: ShellExecutionView = {
  original_input: {
    requires_network: false,
    script: 'exit 7',
    working_directory: '.',
    stdin: '',
    timeout_ms: 1000,
    max_output_bytes: 4096,
  },
  tool_call_id: call,
  command: {
    shell: 'power_shell',
    script: 'exit 7',
    working_directory: '.',
    stdin: '',
    timeout_ms: 1000,
    max_output_bytes: 4096,
  },
  submitted_script: 'bootstrap\nexit 7',
  environment_version: 'aura-shell-environment-v1',
  shell_image: null,
  state: 'completed',
  process_id: 123,
  started_at: '2026-09-09T00:00:00Z',
  finished_at: '2026-09-09T00:00:01Z',
  exit_code: 7,
  stop_reason: 'exited',
  diagnostic: null,
  background: 'running',
  stdout: [97, 98],
  stderr: [],
  stdout_total: 4,
  stderr_total: 0,
  stdout_next: 2,
  stderr_next: null,
  output_truncated: false,
};
beforeEach(() => {
  readShellExecution.mockReset();
  stopShellExecution.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readShellExecution, stopShellExecution } },
  });
});
afterEach(cleanup);
test('direct completion and nonzero exit are independent of background; read more never repeats the command', async () => {
  readShellExecution
    .mockResolvedValueOnce({ ok: true, value: view })
    .mockResolvedValueOnce({ ok: true, value: { ...view, stdout: [99, 100], stdout_next: null } });
  render(<ShellExecutionDetails sessionId={session} callId={call} />);
  expect(await screen.findByText('调用已完成 · 退出码 7')).toBeTruthy();
  expect(screen.getByText(/托管后台进程：仍在运行/)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '读取更多已保存输出' }));
  await screen.findByText('abcd');
  expect(readShellExecution.mock.calls[1]?.[0].input.stdout_offset).toBe(2);
  expect(readShellExecution.mock.calls[1]?.[0].input.tool_call_id).toBe(call);
  expect(stopShellExecution).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText('实际提交的脚本快照'));
  expect(screen.getByText(/bootstrap/)).toBeTruthy();
});
test('waiting and interruption never invent an exit code or keep showing execution', async () => {
  readShellExecution
    .mockResolvedValueOnce({
      ok: true,
      value: {
        ...view,
        state: 'waiting',
        process_id: null,
        started_at: null,
        finished_at: null,
        exit_code: null,
        background: null,
        stdout: [],
        stdout_next: null,
      },
    })
    .mockResolvedValueOnce({
      ok: true,
      value: {
        ...view,
        state: 'interrupted',
        exit_code: null,
        background: null,
        stdout: [],
        stdout_next: null,
      },
    });
  render(<ShellExecutionDetails sessionId={session} callId={call} />);
  await screen.findByText('未完成 · 等待执行');
  expect(screen.queryByText(/退出码/)).toBeNull();
  await waitFor(() =>
    expect(screen.getByRole('button', { name: '刷新状态' }).hasAttribute('disabled')).toBe(false),
  );
  fireEvent.click(screen.getByRole('button', { name: '刷新状态' }));
  await screen.findByText('已中断 · 未正常完成');
  expect(screen.queryByText('未完成 · 等待执行')).toBeNull();
});
