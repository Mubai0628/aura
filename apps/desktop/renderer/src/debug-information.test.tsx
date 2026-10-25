import { render, screen } from '@testing-library/react';
import { beforeEach, expect, test, vi } from 'vitest';
import { App } from './App';
import { installAppTestEnvironment } from './app-test-fixtures';
import { resetUiSettingsForTests } from './ui-settings-store';

installAppTestEnvironment();

// The session's runtime readiness and its tool execution record are for
// debugging. They show only while 设置 → 环境 → 显示调试信息 is on, which it is not by default.
const uiSettings = (show: boolean) =>
  vi.fn().mockResolvedValue({
    ok: true,
    value: {
      revision: 1,
      settings: { show_debug_information: show, default_terminal: 'power_shell' },
      issue: null,
    },
  });
beforeEach(() => resetUiSettingsForTests());

test('a session shows no debugging information by default', async () => {
  Object.assign(window.aura.v1, { getUiSettings: uiSettings(false) });
  render(<App />);
  await screen.findByRole('region', { name: '消息编辑器' });
  await new Promise((resolve) => setTimeout(resolve, 50));
  expect(screen.queryByLabelText('运行能力')).toBeNull();
  // The 近期活动 card reads the last three calls; the debugging record (its pages of 20)
  // never reads.
  expect(window.aura.v1.readAgentToolActivity).not.toHaveBeenCalledWith(
    expect.objectContaining({ limit: 20 }),
  );
});

test('with 显示调试信息 on, the session shows its runtime readiness and tool record', async () => {
  Object.assign(window.aura.v1, { getUiSettings: uiSettings(true) });
  render(<App />);
  expect(await screen.findByLabelText('运行能力')).toBeTruthy();
  await vi.waitFor(() =>
    expect(window.aura.v1.readAgentToolActivity).toHaveBeenCalledWith(
      expect.objectContaining({ limit: 20 }),
    ),
  );
});
