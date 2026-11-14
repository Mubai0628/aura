import { cleanup, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { SessionGrantDialog } from '../security/SessionGrantDialog';
import type { useSecurityControl } from '../security-control-store';
import { SubagentGrantDialog } from './SubagentGrantDialog';

const security = vi.hoisted(
  () => ({ permission: { session_id: 'ses_child' }, loading: false, grants: [] }) as const,
);
vi.mock('../security-control-store', () => ({ useSecurityControl: () => security }));
vi.mock('./team-store', () => ({
  useAgentTeam: () => ({
    members: [
      { agent: 'A1', allowed: { deletions: true, downloads: false, outside_writes: false } },
    ],
  }),
  refreshAgentTeam: () => undefined,
  showAgentTeam: () => undefined,
}));

afterEach(cleanup);

beforeEach(() => {
  Object.assign(window, {
    aura: {
      v1: {
        listNetworkSessionConsents: async () => ({ ok: true, value: [] }),
        listFileSessionConsents: async () => ({ ok: true, value: [] }),
        listMemorySessionConsents: async () => ({ ok: true, value: [] }),
        listNativeSessionConsents: async () => ({ ok: true, value: [] }),
        listExternalGrants: async () => ({ ok: true, value: [] }),
      },
    },
  });
});

test("a sub-agent's grants are named after it while the conversation's own grants dialog stays mounted", async () => {
  // As in App: the conversation's grants dialog is always mounted, closed until opened.
  render(
    <>
      <SessionGrantDialog
        onClose={() => undefined}
        open={false}
        security={security as unknown as ReturnType<typeof useSecurityControl>}
        sessionId="ses_main"
      />
      <SubagentGrantDialog
        target={{
          parentSessionId: 'ses_main',
          agent: 'A1',
          name: '扫雷玩法',
          sessionId: 'ses_child',
          workspaceId: 'wsp_child',
        }}
      />
    </>,
  );
  const dialog = await screen.findByRole('dialog', { name: '扫雷玩法 的授权' });
  const allowances = within(dialog).getByRole('region', { name: '不用问的事' });
  expect(within(allowances).getByText('命令删除它工作副本里的文件')).toBeTruthy();
});
