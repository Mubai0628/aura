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
vi.mock('./agent-settings/AgentTeamSettings', () => ({
  AgentTeamSettings: () => <h2>多 Agent</h2>,
}));
afterEach(cleanup);

const props = (section: ComponentProps<typeof SettingsView>['section']) => ({
  coreReady: false,
  coreStatus: { label: 'Core 未连接', tone: 'muted' as const },
  onClose: vi.fn(),
  onNotify: vi.fn(),
  onWithdrawNotice: vi.fn(),
  section,
  security: {} as ComponentProps<typeof SettingsView>['security'],
});

/** 多 Agent: under 智能能力 after 命令, found by its limits, and its page shown. */
test('多 Agent follows 命令 in 智能能力, the search finds it by token, and it opens its page', async () => {
  const change = vi.fn();
  const { rerender } = render(<SettingsView {...props('models')} onChangeSection={change} />);
  const group = [...document.querySelectorAll('.settings-navigation > *')].map((node) =>
    node.textContent?.trim(),
  );
  const at = group.indexOf('智能能力');
  expect(group.slice(at, at + 4)).toEqual(['智能能力', '记忆', '命令', '多 Agent']);
  await userEvent.click(screen.getByRole('button', { name: '多 Agent' }));
  expect(change).toHaveBeenCalledWith('agents');
  await userEvent.type(screen.getByRole('searchbox'), 'token');
  expect(
    [...document.querySelectorAll('.settings-navigation button')].map((b) => b.textContent),
  ).toEqual(['多 Agent']);
  rerender(<SettingsView {...props('agents')} onChangeSection={change} />);
  expect(screen.getByRole('heading', { name: '多 Agent' })).toBeTruthy();
});
