import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import { HostHarness } from './host-harness';
import { removeTerminal } from './store';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';
import { resetUiSettingsForTests } from '../ui-settings-store';
const engine = vi.hoisted(() => ({
  terminal: { focus: vi.fn(), options: { disableStdin: false } },
}));
vi.mock('./xterm-runtime', () => ({
  terminalEngine: () => engine,
  mountTerminal: () => () => {},
  disposeTerminalEngine: vi.fn(),
}));
const getUiSettings = vi.fn(async () => ({
  ok: true,
  value: {
    revision: 1,
    settings: { show_debug_information: false, default_terminal: 'power_shell' },
    issue: null,
  },
}));
beforeEach(() => {
  resetToolTabsForTests();
  resetUiSettingsForTests();
});

it('takes over and returns an Agent terminal without replacing its screen', async () => {
  const view = {
    session_id: 'session',
    terminal_id: 'agent-control',
    shell: 'cmd',
    process_id: 1,
    started_at: '2026-10-14T00:00:00Z',
    end: null,
    owner: 'agent',
    controller: 'agent',
    title: 'Agent fixture',
    tool_call_id: null,
    revision: '1',
  };
  let revision = 1;
  const control = vi.fn(async (request: { controller: string }) => ({
    ok: true,
    value: { ...view, controller: request.controller, revision: String(++revision) },
  }));
  const api = {
    list: vi.fn(async () => ({ ok: true, value: { terminals: [view], next_cursor: null } })),
    subscribe: vi.fn(async () => ({
      ok: true,
      value: {
        runtime_instance_id: 'runtime',
        session_id: view.session_id,
        terminal_id: view.terminal_id,
        start_offset: 0,
        next_offset: 0,
        data_base64: '',
        truncated: false,
        end: null,
      },
    })),
    control,
    onState: vi.fn(),
    onOutput: vi.fn(),
  };
  vi.stubGlobal('aura', { v1: { terminal: api, onCoreHealth: vi.fn(), getUiSettings } });
  toolTabs.showTerminal('session', view.terminal_id);
  const rendered = render(<HostHarness session="session" />);
  await screen.findByRole('button', { name: '接管' });
  const original = rendered.container.querySelector('.terminal-host');
  expect(engine.terminal.options.disableStdin).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: '接管' }));
  await screen.findByRole('button', { name: '交还' });
  expect(control.mock.calls[0]?.[0]).toEqual({
    session_id: 'session',
    terminal_id: 'agent-control',
    controller: 'user',
  });
  await waitFor(() => expect(engine.terminal.options.disableStdin).toBe(false));
  fireEvent.click(screen.getByRole('button', { name: '交还' }));
  await screen.findByRole('button', { name: '接管' });
  expect(rendered.container.querySelector('.terminal-host')).toBe(original);
  rendered.unmount();
  removeTerminal(view.terminal_id);
  vi.unstubAllGlobals();
});

// A failed takeover's reason stayed on screen after a later one succeeded, and a second
// click while the first was pending sent the opposite request.
it('a takeover failure clears on success, and the button waits for the pending answer', async () => {
  const view = {
    session_id: 'session',
    terminal_id: 'agent-control-retry',
    shell: 'cmd',
    process_id: 1,
    started_at: '2026-10-14T00:00:00Z',
    end: null,
    owner: 'agent',
    controller: 'agent',
    title: 'Agent fixture',
    tool_call_id: null,
    revision: '1',
  };
  let release: (() => void) | undefined;
  const control = vi
    .fn()
    .mockResolvedValueOnce({
      ok: false,
      error: { code: 'terminal.busy', message: 'terminal.busy: 终端正忙，请稍后再试。' },
    })
    .mockImplementationOnce(
      (request: { controller: string }) =>
        new Promise((resolve) => {
          release = () =>
            resolve({
              ok: true,
              value: { ...view, controller: request.controller, revision: '2' },
            });
        }),
    );
  const api = {
    list: vi.fn(async () => ({ ok: true, value: { terminals: [view], next_cursor: null } })),
    subscribe: vi.fn(async () => ({
      ok: true,
      value: {
        runtime_instance_id: 'runtime',
        session_id: view.session_id,
        terminal_id: view.terminal_id,
        start_offset: 0,
        next_offset: 0,
        data_base64: '',
        truncated: false,
        end: null,
      },
    })),
    control,
    onState: vi.fn(),
    onOutput: vi.fn(),
  };
  vi.stubGlobal('aura', { v1: { terminal: api, onCoreHealth: vi.fn(), getUiSettings } });
  toolTabs.showTerminal('session', view.terminal_id);
  const rendered = render(<HostHarness session="session" />);
  fireEvent.click(await screen.findByRole('button', { name: '接管' }));
  expect((await screen.findByRole('alert')).textContent).toBe('终端正忙，请稍后再试。');
  fireEvent.click(screen.getByRole('button', { name: '接管' }));
  await waitFor(() => expect(control).toHaveBeenCalledTimes(2));
  expect(screen.queryByRole('alert')).toBeNull();
  expect((screen.getByRole('button', { name: '接管' }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: '接管' }));
  expect(control).toHaveBeenCalledTimes(2);
  release?.();
  await screen.findByRole('button', { name: '交还' });
  rendered.unmount();
  removeTerminal(view.terminal_id);
  vi.unstubAllGlobals();
});
