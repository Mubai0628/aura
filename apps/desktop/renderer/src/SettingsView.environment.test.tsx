import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import type { ComponentProps } from 'react';
import { SettingsView } from './SettingsView';
vi.mock('./ModelSettings', () => ({ ModelSettings: () => null }));
vi.mock('./ToolSettings', () => ({ ToolSettings: () => null }));
vi.mock('./SecuritySettings', () => ({ SecuritySettings: () => null }));
vi.mock('./settings-recovery/ConfigRecoveryPanel', () => ({ ConfigRecoveryPanel: () => null }));
vi.mock('./ProjectionCompatibilityNotice', () => ({ ProjectionCompatibilityNotice: () => null }));
afterEach(cleanup);

const navigation = () =>
  [...document.querySelectorAll('.settings-navigation button')].map((button) =>
    button.textContent?.trim(),
  );

/** The 环境 page: after 工具 in 基础, and found by what is on it. */
test('环境 follows 工具 and the search finds it by Git or PowerShell', async () => {
  const change = vi.fn();
  render(
    <SettingsView
      coreReady={false}
      coreStatus={{ label: 'Core 未连接', tone: 'muted' }}
      onChangeSection={change}
      onClose={vi.fn()}
      onNotify={vi.fn()}
      onWithdrawNotice={vi.fn()}
      section="tools"
      security={{} as ComponentProps<typeof SettingsView>['security']}
    />,
  );
  expect(navigation().slice(0, 4)).toEqual(['模型', '权限与沙箱', '工具', '环境']);
  await userEvent.click(screen.getByRole('button', { name: '环境' }));
  expect(change).toHaveBeenCalledWith('environment');
  const search = screen.getByRole('searchbox');
  await userEvent.type(search, 'PowerShell');
  expect(navigation()).toEqual(['环境']);
  await userEvent.clear(search);
  await userEvent.type(search, 'git');
  expect(navigation()).toEqual(['环境']);
});
