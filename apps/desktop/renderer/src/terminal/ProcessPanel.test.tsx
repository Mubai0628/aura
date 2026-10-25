import { fireEvent, render, screen, waitFor, cleanup } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { ProcessPanel } from './ProcessPanel';
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
it('keeps an unconfirmed stop visible after a successful automatic refresh', async () => {
  const process = {
    kind: 'terminal',
    command: 'running fixture',
    started_at: '2026-10-14T14:00:00Z',
    tool_call_id: 'call-1',
    terminal_id: 'term-1',
  };
  const listProcesses = vi.fn().mockResolvedValue({ ok: true, value: { processes: [process] } });
  const stopProcess = vi.fn().mockResolvedValue({ ok: true, value: { stopped: false } });
  vi.stubGlobal('aura', { v1: { terminal: { listProcesses, stopProcess } } });
  render(<ProcessPanel sessionId="session-1" onViewTerminal={vi.fn()} />);
  await screen.findByText('running fixture');
  fireEvent.click(screen.getByRole('button', { name: '停止' }));
  await waitFor(() => expect(listProcesses).toHaveBeenCalledTimes(2));
  expect(screen.getByRole('alert').textContent).toContain('停止尚未确认');
});
it('views the existing terminal and stops only the selected owned process', async () => {
  const process = {
    kind: 'terminal',
    command: 'fixture command',
    started_at: '2026-10-14T14:00:00Z',
    tool_call_id: 'call-1',
    terminal_id: 'term-1',
  };
  const listProcesses = vi
    .fn()
    .mockResolvedValueOnce({ ok: true, value: { processes: [process] } })
    .mockResolvedValue({ ok: true, value: { processes: [] } });
  const stopProcess = vi.fn().mockResolvedValue({ ok: true, value: { stopped: true } });
  const view = vi.fn();
  vi.stubGlobal('aura', { v1: { terminal: { listProcesses, stopProcess } } });
  render(<ProcessPanel sessionId="session-1" onViewTerminal={view} />);
  await screen.findByText('fixture command');
  // A model terminal opens as a tab of the sidebar, so its button says so.
  fireEvent.click(screen.getByRole('button', { name: '查看终端' }));
  expect(view).toHaveBeenCalledWith('term-1');
  fireEvent.click(screen.getByRole('button', { name: '停止' }));
  await waitFor(() =>
    expect(stopProcess).toHaveBeenCalledWith({ session_id: 'session-1', tool_call_id: 'call-1' }),
  );
  await screen.findByText('本会话没有仍在运行的后台进程。');
});
