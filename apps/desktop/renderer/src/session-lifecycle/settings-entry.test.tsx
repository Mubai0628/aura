import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import type { ComponentProps } from 'react';
import { SettingsView } from '../SettingsView';
vi.mock('../ModelSettings', () => ({ ModelSettings: () => null }));
vi.mock('../ToolSettings', () => ({ ToolSettings: () => null }));
vi.mock('../SecuritySettings', () => ({ SecuritySettings: () => null }));
vi.mock('../settings-recovery/ConfigRecoveryPanel', () => ({ ConfigRecoveryPanel: () => null }));
vi.mock('../ProjectionCompatibilityNotice', () => ({ ProjectionCompatibilityNotice: () => null }));
vi.mock('./ArchiveSettings', () => ({ ArchiveSettings: () => <p>归档页</p> }));
afterEach(cleanup);

const navigation = () =>
  [...document.querySelectorAll('.settings-navigation button')].map((button) =>
    button.textContent?.trim(),
  );

test('归档 remains under 系统 before About and Updates, and search finds its actions', async () => {
  const change = vi.fn();
  const props: ComponentProps<typeof SettingsView> = {
    coreReady: false,
    coreStatus: { label: 'Core 未连接', tone: 'muted' },
    onChangeSection: change,
    onClose: vi.fn(),
    onNotify: vi.fn(),
    onWithdrawNotice: vi.fn(),
    section: 'tools',
    security: {} as ComponentProps<typeof SettingsView>['security'],
  };
  const { rerender } = render(<SettingsView {...props} />);
  expect(navigation().slice(-3)).toEqual(['归档', '通知', '关于与更新']);
  expect(screen.getByText('系统')).toBeTruthy();
  await userEvent.click(screen.getByRole('button', { name: '归档' }));
  expect(change).toHaveBeenCalledWith('archive');
  rerender(<SettingsView {...props} section="archive" />);
  expect(screen.getByText('归档页')).toBeTruthy();
  const search = screen.getByRole('searchbox');
  for (const word of ['删除', '恢复']) {
    await userEvent.clear(search);
    await userEvent.type(search, word);
    expect(navigation()).toEqual(['归档']);
  }
});
