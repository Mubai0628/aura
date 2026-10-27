import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';

import {
  DEFAULT_NOTIFICATION_PREFERENCES,
  type NotificationPreferencesCommand,
} from '../../../shared/notifications';
import { NotificationSettings } from './NotificationSettings';

// 设置 → 通知: one switch per kind of notification, all on until the user turns one off, kept by
// Main.

afterEach(cleanup);

function bridge() {
  let saved = { ...DEFAULT_NOTIFICATION_PREFERENCES };
  const preferences = vi.fn(async (command: NotificationPreferencesCommand) => {
    if (command.kind === 'set') saved = command.preferences;
    return saved;
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { notifications: { notificationPreferences: preferences } } },
  });
  return preferences;
}

test('every kind of notification is on until turned off, and a change is kept', async () => {
  const preferences = bridge();
  render(<NotificationSettings />);
  const completions = await screen.findByRole('switch', { name: '任务完成' });
  for (const name of ['有新版本', '需要审批', '模型提问或等你决定', '任务完成', '任务出错或暂停'])
    expect(screen.getByRole('switch', { name }).getAttribute('aria-checked')).toBe('true');
  fireEvent.click(completions);
  await vi.waitFor(() =>
    expect(preferences).toHaveBeenLastCalledWith({
      kind: 'set',
      preferences: { ...DEFAULT_NOTIFICATION_PREFERENCES, completions: false },
    }),
  );
  await vi.waitFor(() => expect(completions.getAttribute('aria-checked')).toBe('false'));
});
