import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { EnableDebuggingOffer } from './EnableDebuggingOffer';

// On the row of a page-debugging call the switch stopped, the user turns it on
// after the same confirmation as in the settings; nothing changes until they confirm.

const getSettings = vi.fn();
const updateSettings = vi.fn();
const settings = { enabled: true, allowed_sites: [], blocked_sites: [], debugging: false };

beforeEach(() => {
  getSettings
    .mockReset()
    .mockResolvedValue({ ok: true, value: { revision: '4', settings, issue: null } });
  updateSettings.mockReset().mockResolvedValue({ ok: true, value: {} });
  vi.stubGlobal('aura', { v1: { browser: { getSettings, updateSettings } } });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test('the switch is turned on only after the user confirms', async () => {
  const user = userEvent.setup();
  render(<EnableDebuggingOffer />);
  await user.click(screen.getByRole('button', { name: '允许模型调试网页' }));
  expect(screen.getByRole('dialog', { name: '允许模型调试网页？' })).toBeTruthy();
  expect(updateSettings).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: '允许' }));
  await waitFor(() =>
    expect(updateSettings).toHaveBeenCalledWith({
      expected_revision: '4',
      settings: { ...settings, debugging: true },
    }),
  );
  expect(await screen.findByText('已允许模型调试网页')).toBeTruthy();
});

test('cancelling changes nothing', async () => {
  const user = userEvent.setup();
  render(<EnableDebuggingOffer />);
  await user.click(screen.getByRole('button', { name: '允许模型调试网页' }));
  await user.click(screen.getByRole('button', { name: '取消' }));
  expect(updateSettings).not.toHaveBeenCalled();
  expect(getSettings).not.toHaveBeenCalled();
});
