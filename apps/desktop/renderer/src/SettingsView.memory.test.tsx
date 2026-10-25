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
test('memory is a real settings section available from the existing navigation', async () => {
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
  await userEvent.click(screen.getByRole('button', { name: '记忆' }));
  expect(change).toHaveBeenCalledWith('memory');
});
