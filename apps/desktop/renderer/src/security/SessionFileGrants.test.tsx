import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { SessionGrantDialog } from './SessionGrantDialog';
import type { useSecurityControl } from '../security-control-store';

afterEach(cleanup);
test('S06 F2 shows persistent file scope and withdraws the exact revision', async () => {
  const grant = {
    grant_id: 'file-grant',
    grant_revision: '2',
    availability: 'in_force',
    scope: {
      session_id: 'session',
      capabilities: ['workspace_file_write'],
      targets: [{ canonical_path: 'C:\\fixture\\note.txt', recursive: false }],
    },
  };
  const list = vi.fn().mockResolvedValue({ ok: true, value: [grant] });
  const revoke = vi.fn(async () => {
    list.mockResolvedValue({ ok: true, value: [{ ...grant, availability: 'revoked' }] });
    return { ok: true, value: { ...grant, availability: 'revoked' } };
  });
  Object.assign(window, {
    aura: {
      v1: {
        listNativeSessionConsents: async () => ({ ok: true, value: [] }),
        listExternalGrants: async () => ({ ok: true, value: [] }),
        listFileSessionConsents: list,
        listMemorySessionConsents: async () => ({ ok: true, value: [] }),
        listNetworkSessionConsents: async () => ({ ok: true, value: [] }),
        revokeFileSessionConsent: revoke,
      },
    },
  });
  const security = {
    permission: { session_id: 'session' },
    loading: false,
    grants: [],
  } as unknown as ReturnType<typeof useSecurityControl>;
  render(<SessionGrantDialog security={security} sessionId="session" onClose={() => undefined} />);
  expect(await screen.findByText(/C:\\fixture\\note.txt/)).toBeTruthy();
  // The section lists the approvals; their lifetime rules were shown in the approval
  // card's details when they were given.
  const files = within(screen.getByRole('region', { name: '会话文件批准' }));
  expect(files.queryByText(/模式变化和 Core 重启后仍有效|被其他程序替换时/u)).toBeNull();
  await userEvent.setup().click(screen.getByRole('button', { name: '撤销文件批准' }));
  await waitFor(() =>
    expect(revoke).toHaveBeenCalledWith({
      session_id: 'session',
      grant_id: 'file-grant',
      expected_revision: '2',
    }),
  );
  expect(await screen.findByText(/已撤销/)).toBeTruthy();
});
