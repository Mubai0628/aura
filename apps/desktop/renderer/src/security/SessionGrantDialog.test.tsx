import { useState } from 'react';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test } from 'vitest';
import { isP04MainParams } from '../../../shared/p04-validation';
import { SessionGrantDialog } from './SessionGrantDialog';
import type { useSecurityControl } from '../security-control-store';
afterEach(cleanup);

/**
 * The Native session consent route is a separate management entry, so every case
 * has to stub it. A missing stub would make the dialog crash rather than exercise
 * the behaviour the case is about, which is why the default here is an empty list
 * instead of "absent".
 */
const consentStub = {
  listNetworkSessionConsents: async () => ({ ok: true, value: [] }),
  listFileSessionConsents: async () => ({ ok: true, value: [] }),
  listMemorySessionConsents: async () => ({ ok: true, value: [] }),
  listNativeSessionConsents: async () => ({ ok: true, value: [] }),
  revokeNativeSessionConsent: async () => ({ ok: true, value: null }),
};

beforeEach(() => {
  Object.assign(window, {
    aura: {
      v1: {
        ...consentStub,
        listExternalGrants: async () => ({ ok: true, value: [] }),
        revokeExternalGrant: async () => ({ ok: true, value: null }),
      },
    },
  });
});
function Fixture() {
  const [open, setOpen] = useState(false);
  const security = {
    permission: { session_id: 'session' },
    loading: false,
    grants: [],
  } as unknown as ReturnType<typeof useSecurityControl>;
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        会话授权管理
      </button>
      {open && (
        <SessionGrantDialog
          security={security}
          sessionId="session"
          onClose={() => setOpen(false)}
        />
      )}
    </>
  );
}
test.each(['close', 'backdrop', 'escape'])(
  'grant management closes using %s and returns focus',
  async (method) => {
    const user = userEvent.setup();
    render(<Fixture />);
    const trigger = screen.getByRole('button', { name: '会话授权管理' });
    await user.click(trigger);
    const dialog = screen.getByRole('dialog', { name: '会话授权管理' });
    expect(dialog).toBeTruthy();
    if (method === 'close') await user.click(screen.getByRole('button', { name: '关闭授权管理' }));
    else if (method === 'backdrop') fireEvent.mouseDown(dialog.closest('.modal-backdrop')!);
    else await user.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  },
);

test('switching sessions does not keep the previous directory grants', async () => {
  let release: (() => void) | undefined;
  Object.assign(window, {
    aura: {
      v1: {
        ...consentStub,
        listExternalGrants: ({ session_id }: { session_id: string }) => {
          if (session_id === 'session-a') {
            return Promise.resolve({
              ok: true,
              value: [
                {
                  id: 'grant-a',
                  display_path: 'C:\\only-a',
                  target_kind: 'directory',
                  recursive_scope: false,
                  lifecycle: 'active',
                  capabilities: ['read'],
                  term: 'session',
                  availability: 'in_force',
                },
              ],
            });
          }
          return new Promise((resolve) => {
            release = () => resolve({ ok: true, value: [] });
          });
        },
        revokeExternalGrant: async () => ({ ok: true, value: null }),
      },
    },
  });
  function Switcher() {
    const [sessionId, setSessionId] = useState('session-a');
    const security = {
      permission: { session_id: sessionId },
      loading: false,
      grants: [],
    } as unknown as ReturnType<typeof useSecurityControl>;
    return (
      <>
        <button type="button" onClick={() => setSessionId('session-b')}>
          切换会话
        </button>
        <SessionGrantDialog security={security} sessionId={sessionId} onClose={() => undefined} />
      </>
    );
  }
  render(<Switcher />);
  expect(await screen.findByText(/only-a/)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '切换会话' }));
  expect(screen.queryByText(/only-a/)).toBeNull();
  // Scoped to the directory list: the Native consent section has its own loading
  // status, and a switch has to clear both.
  expect(screen.getByText('正在读取授权…')).toBeTruthy();
  release?.();
});

test('a failed directory grant read is not shown as an empty list', async () => {
  Object.assign(window, {
    aura: {
      v1: {
        ...consentStub,
        listExternalGrants: async () => ({ ok: false, error: { message: 'unavailable' } }),
        revokeExternalGrant: async () => ({ ok: true, value: null }),
      },
    },
  });
  const security = {
    permission: { session_id: 'session' },
    loading: false,
    grants: [],
  } as unknown as ReturnType<typeof useSecurityControl>;
  render(<SessionGrantDialog security={security} sessionId="session" onClose={() => undefined} />);
  expect(await screen.findByRole('alert')).toBeTruthy();
  expect(screen.queryByText('当前没有可撤销的持久授权。')).toBeNull();
});

test('an in-force Native session consent is listed and can be withdrawn', async () => {
  const revoked: string[] = [];
  const commands: Array<{ grant_id: string; correlation_id: string }> = [];
  Object.assign(window, {
    aura: {
      v1: {
        ...consentStub,
        listExternalGrants: async () => ({ ok: true, value: [] }),
        revokeExternalGrant: async () => ({ ok: true, value: null }),
        listNativeSessionConsents: async () => ({
          ok: true,
          value: [
            {
              grant_id: 'grant-native',
              grant_revision: '1',
              approval_id: 'approval-native',
              scope: {
                session_id: 'session',
                workspace_id: 'workspace',
                approved_shell: 'powershell',
                shells: ['powershell', 'cmd'],
                principal_id: 'principal',
                approved_permission_revision: 1,
                core_runtime_instance_id: 'runtime',
                approved_operation_fingerprint: { algorithm: 'sha256', hex: 'a'.repeat(64) },
              },
              availability: 'in_force',
              issued_at: '2026-09-27T00:00:00Z',
              approved_through: '2026-09-27T00:10:00Z',
              revoked_at: null,
              backend_id: 'windows-user-native-v1',
            },
          ],
        }),
        revokeNativeSessionConsent: async (request: {
          grant_id: string;
          correlation_id: string;
        }) => {
          commands.push(request);
          revoked.push(request.grant_id);
          return { ok: true, value: null };
        },
      },
    },
  });
  const security = {
    permission: { session_id: 'session' },
    loading: false,
    grants: [],
  } as unknown as ReturnType<typeof useSecurityControl>;
  render(<SessionGrantDialog security={security} sessionId="session" onClose={() => undefined} />);
  const withdraw = await screen.findByRole('button', { name: '撤销 Shell 运行批准' });
  expect(screen.getByText(/PowerShell 与 CMD/)).toBeTruthy();
  expect(screen.getByText(/首次批准：PowerShell/)).toBeTruthy();
  // The dialog says only what a withdrawal reaches; no explanations.
  expect(
    screen.queryByText(/管理当前会话可使用的授权|按旧规则保留为历史记录|模式切换后仍有效/u),
  ).toBeNull();
  expect(screen.getByText('撤销工作区授权也会影响该工作区的其他会话。')).toBeTruthy();
  fireEvent.click(withdraw);
  expect(revoked).toEqual(['grant-native']);
  expect(
    isP04MainParams('security.native_session_consent.revoke', {
      ...commands[0],
      session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      grant_id: 'pgr_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    }),
    'the real Preload accepts the generated withdrawal identity',
  ).toBe(true);
});

test('a consent approved for one Shell names the whole covered range', async () => {
  Object.assign(window, {
    aura: {
      v1: {
        ...consentStub,
        listExternalGrants: async () => ({ ok: true, value: [] }),
        revokeExternalGrant: async () => ({ ok: true, value: null }),
        listNativeSessionConsents: async () => ({
          ok: true,
          value: [
            {
              grant_id: 'grant-native',
              grant_revision: '1',
              approval_id: 'approval-native',
              scope: {
                session_id: 'session',
                workspace_id: 'workspace',
                approved_shell: 'cmd',
                shells: ['powershell', 'cmd'],
                principal_id: 'principal',
                approved_permission_revision: 1,
                core_runtime_instance_id: 'runtime',
                approved_operation_fingerprint: { algorithm: 'sha256', hex: 'a'.repeat(64) },
              },
              availability: 'in_force',
              issued_at: '2026-09-27T00:00:00Z',
              approved_through: '2026-09-27T00:10:00Z',
              revoked_at: null,
              backend_id: 'windows-user-native-v1',
            },
          ],
        }),
      },
    },
  });
  const security = {
    permission: { session_id: 'session' },
    loading: false,
    grants: [],
  } as unknown as ReturnType<typeof useSecurityControl>;
  render(<SessionGrantDialog security={security} sessionId="session" onClose={() => undefined} />);
  expect(await screen.findByText(/PowerShell 与 CMD/)).toBeTruthy();
  // The tool the user confirmed is named separately from the range it covers.
  expect(screen.getByText(/首次批准：CMD/)).toBeTruthy();
});

test('a revoked Native session consent is shown without a withdrawal button', async () => {
  Object.assign(window, {
    aura: {
      v1: {
        ...consentStub,
        listExternalGrants: async () => ({ ok: true, value: [] }),
        revokeExternalGrant: async () => ({ ok: true, value: null }),
        listNativeSessionConsents: async () => ({
          ok: true,
          value: [
            {
              grant_id: 'grant-native',
              grant_revision: '2',
              approval_id: 'approval-native',
              scope: {
                session_id: 'session',
                workspace_id: 'workspace',
                approved_shell: 'cmd',
                shells: ['powershell', 'cmd'],
                principal_id: 'principal',
                approved_permission_revision: 1,
                core_runtime_instance_id: 'runtime',
                approved_operation_fingerprint: { algorithm: 'sha256', hex: 'a'.repeat(64) },
              },
              availability: 'revoked',
              issued_at: '2026-09-27T00:00:00Z',
              approved_through: '2026-09-27T00:10:00Z',
              revoked_at: '2026-09-27T00:05:00Z',
              backend_id: 'windows-user-native-v1',
            },
          ],
        }),
      },
    },
  });
  const security = {
    permission: { session_id: 'session' },
    loading: false,
    grants: [],
  } as unknown as ReturnType<typeof useSecurityControl>;
  render(<SessionGrantDialog security={security} sessionId="session" onClose={() => undefined} />);
  expect(await screen.findByText(/已撤销/)).toBeTruthy();
  expect(screen.queryByRole('button', { name: '撤销 Shell 运行批准' })).toBeNull();
});

test('a consent from an earlier Core run reads as superseded, never as pending', async () => {
  // The restarted Core is a state the person has to be able to tell apart from "not yet
  // in effect": the first ends the consent, the second says it is about to start. Showing
  // the first as the second is how a dead authorization looked alive.
  Object.assign(window, {
    aura: {
      v1: {
        ...consentStub,
        listExternalGrants: async () => ({ ok: true, value: [] }),
        revokeExternalGrant: async () => ({ ok: true, value: null }),
        listNativeSessionConsents: async () => ({
          ok: true,
          value: [
            {
              grant_id: 'grant-native',
              grant_revision: '1',
              approval_id: 'approval-native',
              scope: {
                session_id: 'session',
                workspace_id: 'workspace',
                approved_shell: 'powershell',
                shells: ['powershell', 'cmd'],
                principal_id: 'principal',
                approved_permission_revision: 1,
                core_runtime_instance_id: 'runtime',
                approved_operation_fingerprint: { algorithm: 'sha256', hex: 'a'.repeat(64) },
              },
              availability: 'superseded',
              issued_at: '2026-09-27T00:00:00Z',
              approved_through: '2026-09-27T00:10:00Z',
              revoked_at: null,
              backend_id: 'windows-user-native-v1',
            },
          ],
        }),
      },
    },
  });
  const security = {
    permission: { session_id: 'session' },
    loading: false,
    grants: [],
  } as unknown as ReturnType<typeof useSecurityControl>;
  render(<SessionGrantDialog security={security} sessionId="session" onClose={() => undefined} />);
  expect(await screen.findByText(/已失效（Core 已重启）/)).toBeTruthy();
  expect(screen.queryByText('尚未生效')).toBeNull();
  expect(screen.queryByRole('button', { name: '撤销 Shell 运行批准' })).toBeNull();
});
