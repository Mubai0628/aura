import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import type { ToolDefinitionInstanceView } from '@aura/contracts/generated';
import { ToolCredentialBindingControl } from './ToolCredentialBindingControl';

afterEach(cleanup);
test('changed credential requirements explain the reason and the rebinding action', () => {
  const instance = {
    tool_instance_id: 'tin_01AAAAAAAAAAAAAAAAAAAAAAAA',
    entity_revision: '1',
    credential_slot_binding: {
      slot_key: 'api_key',
      placement: { placement: 'http_bearer_authorization' },
      bound_credential: null,
      rebind_required_reason: 'credential_slot_changed',
    },
  } as ToolDefinitionInstanceView;
  render(
    <ToolCredentialBindingControl
      boundCredential={null}
      busyAction={undefined}
      credential={instance.credential_slot_binding!}
      instance={instance}
      mutable
      runMutation={vi.fn()}
    />,
  );
  expect(screen.getByRole('alert').textContent).toContain('凭据槽或放置方式已变化');
  expect(screen.getByRole('alert').textContent).toContain('重新绑定凭据');
  expect(screen.getByRole('button', { name: '读取可用 Credential' })).toBeDefined();
});
