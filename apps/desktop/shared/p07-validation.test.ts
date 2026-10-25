import { describe, expect, test } from 'vitest';

import {
  isP07MainCommand,
  isP07MainParams,
  isToolMutationResult,
  type P07MainCommand,
} from './p07-validation.js';

const ID_BODY = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const id = (prefix: string): string => `${prefix}_${ID_BODY}`;
const sha256 = {
  algorithm: 'sha256',
  canonicalization_version: 'rfc8785_jcs_v1',
  hex: '1'.repeat(64),
} as const;

const validRequests: ReadonlyArray<readonly [P07MainCommand, unknown]> = [
  ['tools.tool.set_enabled', { enabled: true, expected_entity_revision: '1', tool_id: id('tol') }],
  [
    'tools.instance.update_configuration',
    {
      configuration: { mode: 'strict' },
      expected_configuration_revision: null,
      expected_entity_revision: '1',
      expected_schema_hash: sha256,
      tool_instance_id: id('tin'),
    },
  ],
  [
    'tools.instance.bind_credential',
    {
      credential_id: id('crd'),
      credential_revision: '1',
      credential_slot_key: 'api_key',
      expected_binding_revision: null,
      expected_candidate_set_revision: '1',
      expected_entity_revision: '1',
      placement: { environment_variable: 'AURA_FIXTURE_API_KEY', placement: 'command_environment' },
      session_id: null,
      tool_instance_id: id('tin'),
      workspace_id: null,
    },
  ],
  [
    'tools.instance.clear_credential',
    {
      expected_binding_revision: '1',
      expected_entity_revision: '1',
      tool_instance_id: id('tin'),
    },
  ],
  [
    'tools.pin.workspace.set',
    {
      expected_entity_revision: null,
      tool_id: id('tol'),
      tool_version: { definition_revision: '1', semantic_version: '1.2.3' },
      workspace_id: id('wsp'),
    },
  ],
  [
    'tools.pin.workspace.clear',
    { expected_entity_revision: '1', tool_id: id('tol'), workspace_id: id('wsp') },
  ],
  [
    'tools.pin.session.set',
    {
      expected_entity_revision: null,
      tool_id: id('tol'),
      tool_version: { definition_revision: '1', semantic_version: null },
      session_id: id('ses'),
    },
  ],
  [
    'tools.pin.session.clear',
    { expected_entity_revision: '1', session_id: id('ses'), tool_id: id('tol') },
  ],
];

describe('P07 Renderer/Main validation', () => {
  test('keeps the authenticated mutation surface closed to exactly 8 methods', () => {
    expect(validRequests).toHaveLength(8);
    for (const [command, request] of validRequests) {
      expect(isP07MainCommand(command)).toBe(true);
      expect(isP07MainParams(command, request), command).toBe(true);
    }
    expect(isP07MainCommand('tools.activity.page')).toBe(false);
    expect(isP07MainCommand('tools.package.install')).toBe(false);
    // Retired the P07 Draft, package and long-operation commands.
    for (const retired of [
      'tools.draft.create',
      'tools.package.set_trust',
      'tools.operation.cancel',
    ]) {
      expect(isP07MainCommand(retired), retired).toBe(false);
    }
  });

  test('rejects unknown fields and non-canonical revisions', () => {
    expect(
      isP07MainParams('tools.tool.set_enabled', {
        enabled: true,
        expected_entity_revision: '1',
        tool_id: id('tol'),
        actor: 'user',
      }),
    ).toBe(false);
    expect(
      isP07MainParams('tools.tool.set_enabled', {
        enabled: true,
        expected_entity_revision: 1,
        tool_id: id('tol'),
      }),
    ).toBe(false);
  });

  test('accepts only the closed mutation result union', () => {
    expect(
      isToolMutationResult({
        draft_revision: '1',
        entity_revision: '2',
        result_kind: 'draft_committed',
        snapshot_hash: sha256,
        tool_draft_id: id('tdr'),
      }),
    ).toBe(true);
    expect(
      isToolMutationResult({
        accepted: {
          operation: {
            journal_id: id('jnl'),
            operation_kind: 'draft_apply',
            tool_draft_id: id('tdr'),
          },
          operational_revision: '1',
          state: 'queued',
        },
        result_kind: 'long_operation_accepted',
      }),
    ).toBe(true);
    expect(
      isToolMutationResult({
        entity_revision: '2',
        result_kind: 'operation_accepted',
        tool_draft_id: id('tdr'),
      }),
    ).toBe(false);
  });
});
