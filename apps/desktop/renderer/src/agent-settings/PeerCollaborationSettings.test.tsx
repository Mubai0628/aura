import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { PeerCollaborationSettings } from './PeerCollaborationSettings';

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test('budget and confirmation are both editable without selecting a session', async () => {
  const user = userEvent.setup();
  let enabled = false;
  const state = () => ({
    collaboration_enabled: true,
    budget_default: { limit: 32, revision: 2 },
    review_policy: { required: enabled, revision: enabled ? 2 : 1 },
    entries: [],
  });
  const read = vi.fn(async () => ({
    ok: true,
    value: state(),
  }));
  const manage = vi.fn(async ({ action }: { action: { kind: string } }) => {
    if (action.kind === 'set_review') enabled = true;
    return { ok: true, value: state() };
  });
  vi.stubGlobal('aura', { v1: { readPeerMailbox: read, managePeerMailbox: manage } });
  render(<PeerCollaborationSettings coreReady />);
  const input = await screen.findByRole('spinbutton', { name: '新协作默认额度' });
  await user.clear(input);
  await user.type(input, '37');
  await user.click(screen.getByRole('button', { name: '保存额度' }));
  await waitFor(() =>
    expect(manage).toHaveBeenCalledWith({
      session_id: null,
      view: 'settings',
      action: { kind: 'set_default', expected_revision: 2, limit: 37 },
    }),
  );
  expect(screen.queryByRole('combobox', { name: '接收会话' })).toBeNull();
  const control = await screen.findByRole('switch', { name: '新投递先由我确认' });
  await waitFor(() => expect(control).toHaveProperty('disabled', false));
  expect(control.getAttribute('aria-checked')).toBe('false');
  await user.click(control);
  await waitFor(() =>
    expect(manage).toHaveBeenCalledWith({
      session_id: null,
      view: 'settings',
      action: { kind: 'set_review', expected_revision: 1, required: true },
    }),
  );
  await waitFor(() =>
    expect(
      screen.getByRole('switch', { name: '新投递先由我确认' }).getAttribute('aria-checked'),
    ).toBe('true'),
  );
  expect(screen.queryByText('同级协作收件箱')).toBeNull();
});
