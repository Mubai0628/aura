import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';

import { ConnectionListView } from './ConnectionListView';
import type { ProviderConnection } from './useProviderConnection';

// : the default is the one chosen, else the first created; every other is a backup.

afterEach(cleanup);

const A = 'end_01ARZ3NDEKTSV4RRFFQ69G5FA1';
const B = 'end_01ARZ3NDEKTSV4RRFFQ69G5FA2';
const C = 'end_01ARZ3NDEKTSV4RRFFQ69G5FA3';
const NAMES: Record<string, string> = { [A]: '主连接', [B]: '备用连接', [C]: '第三个' };

function view(defaultId: string | null, ids = [A, B, C]) {
  const setConnections = vi.fn(async () => undefined);
  const endpoint = (id: string) => ({
    id,
    display_name: NAMES[id],
    api_format: 'open_ai_responses',
    auth_scheme: 'open_ai_bearer_api_key',
    updated_at: id === A ? '2026-10-24T06:05:00Z' : '2026-10-23T06:30:00Z',
  });
  const connection = {
    provider: {
      id: 'prv',
      localProvider: {
        id: 'prv_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        display_name: 'DeepSeek',
        default_endpoint_id: defaultId,
        endpoint_order: ids,
      },
      endpoints: ids.map(endpoint),
    },
    settings: { active_credentials: [], dormant_credentials: [] },
    busy: false,
    writesDisabled: false,
    setPendingDeletion: vi.fn(),
    setProviderDisplayNameDraft: vi.fn(),
    addConnectionButtonRef: { current: null },
    deletionTriggerRef: { current: null },
    providerDisplayName: 'DeepSeek',
    beginEditing: vi.fn(),
    renameProvider: vi.fn(),
    setConnections,
  } as unknown as ProviderConnection;
  render(<ConnectionListView connection={connection} />);
  return setConnections;
}

const row = (id: string) =>
  screen.getByRole('button', { name: new RegExp(`^${NAMES[id]}`, 'u') }).closest('div')!;

test('the first created is the default and every other a backup, with no button to make one', () => {
  view(null);
  expect(within(row(A)).getByText('默认')).toBeTruthy();
  expect(within(row(B)).getByText('备用 1')).toBeTruthy();
  expect(within(row(C)).getByText('备用 2')).toBeTruthy();
  expect(screen.queryByRole('button', { name: /作为备用|不作备用|取消默认/u })).toBeNull();
  expect(within(row(A)).queryByRole('button', { name: '设为默认' })).toBeNull();
  expect(screen.getAllByRole('button', { name: '设为默认' })).toHaveLength(2);
});

test('another connection becomes the default; the others stay backups in the order created', async () => {
  const user = userEvent.setup();
  const set = view(C);
  expect(within(row(C)).getByText('默认')).toBeTruthy();
  expect(within(row(A)).getByText('备用 1')).toBeTruthy();
  expect(within(row(B)).getByText('备用 2')).toBeTruthy();
  await user.click(within(row(B)).getByRole('button', { name: '设为默认' }));
  expect(set).toHaveBeenLastCalledWith(B, []);
});

test('a single connection is the default', () => {
  view(null, [A]);
  expect(within(row(A)).getByText('默认')).toBeTruthy();
  expect(screen.queryByRole('button', { name: '设为默认' })).toBeNull();
});

test('each connection says when it was last changed', () => {
  view(null);
  expect(within(row(A)).getByText(/^修改于 2026\/10\/24 \d\d:\d\d$/u)).toBeTruthy();
  expect(within(row(B)).getByText(/^修改于 2026\/10\/23 \d\d:\d\d$/u)).toBeTruthy();
});
