import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { UiSettings } from '@aura/contracts/generated';
import pinText from '../../../../../runtimes/powershell.json?raw';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { BUNDLED_POWERSHELL } from './bundled-shells';
import { EnvironmentSettings } from './EnvironmentSettings';
import { resetUiSettingsForTests } from '../ui-settings-store';

let revision = 4;
const update = vi.fn();

/** The 环境 page: the local Git and the shells Aura runs, in one place. */
beforeEach(() => {
  resetUiSettingsForTests();
  revision = 4;
  update
    .mockReset()
    .mockImplementation(({ settings }: { expected_revision: number; settings: UiSettings }) =>
      Promise.resolve({ ok: true, value: { revision: ++revision, settings, issue: null } }),
    );
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getUiSettings: vi.fn().mockResolvedValue({
          ok: true,
          value: {
            revision: 4,
            settings: { show_debug_information: false, default_terminal: 'power_shell' },
            issue: null,
          },
        }),
        updateUiSettings: update,
        git: {
          runtime: vi.fn().mockResolvedValue({
            ok: true,
            value: {
              state: 'available',
              executable: 'C:\\Program Files\\Git\\mingw64\\bin\\git.exe',
              version: 'git version 2.47.1',
              message: '本机 Git 可用',
              next_step: '更换或升级 Git 后可重新检测',
              installation_url: 'https://git-scm.com/download/win',
            },
          }),
          openInstallation: vi.fn(),
        },
      },
    },
  });
});
afterEach(cleanup);

test('the environment page shows the local Git and the shells Aura runs', async () => {
  render(<EnvironmentSettings />);
  expect(screen.getByRole('heading', { level: 2, name: '环境' })).toBeTruthy();
  const git = screen.getByRole('region', { name: 'Git 运行环境' });
  expect(
    await within(git).findByText('C:\\Program Files\\Git\\mingw64\\bin\\git.exe'),
  ).toBeTruthy();
  expect(within(git).getByText('git version 2.47.1')).toBeTruthy();
  // An available Git needs no next step.
  expect(within(git).queryByText('更换或升级 Git 后可重新检测')).toBeNull();
  const shells = screen.getByRole('region', { name: '终端与命令' });
  expect(within(shells).getByText(`PowerShell ${BUNDLED_POWERSHELL.version}`)).toBeTruthy();
  expect(within(shells).getByText('随 Aura 安装')).toBeTruthy();
  expect(within(shells).getByText('CMD', { selector: 'dt' })).toBeTruthy();
  expect(within(shells).getByText('Windows 自带')).toBeTruthy();
});

test('the PowerShell version shown is the one Aura pins and verifies at start', () => {
  const pin = JSON.parse(pinText) as { version: string; architecture: string };
  expect(BUNDLED_POWERSHELL).toEqual({ version: pin.version, architecture: pin.architecture });
});

// The terminal a terminal tab opens and whether the session shows its
// debugging information are chosen here, and saved at the revision the page showed.
test('the default terminal and the debugging information are chosen on this page', async () => {
  const user = userEvent.setup();
  render(<EnvironmentSettings />);
  const terminal = await screen.findByRole('radiogroup', { name: '默认终端' });
  await vi.waitFor(() =>
    expect(within(terminal).getByRole('radio', { name: 'PowerShell 7' })).toHaveProperty(
      'checked',
      true,
    ),
  );
  await user.click(within(terminal).getByRole('radio', { name: 'CMD' }));
  expect(update).toHaveBeenLastCalledWith({
    expected_revision: 4,
    settings: { show_debug_information: false, default_terminal: 'cmd' },
  });
  const debug = screen.getByRole('switch', { name: '显示调试信息' });
  expect(debug.getAttribute('aria-checked')).toBe('false');
  await user.click(debug);
  expect(update).toHaveBeenLastCalledWith({
    expected_revision: 5,
    settings: { show_debug_information: true, default_terminal: 'cmd' },
  });
  await vi.waitFor(() => expect(debug.getAttribute('aria-checked')).toBe('true'));
});
