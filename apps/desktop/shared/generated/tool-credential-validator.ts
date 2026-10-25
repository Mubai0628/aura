// @ts-nocheck
/* eslint-disable */
/** Generated from Rust routes and schemas. Do not edit. */
/* AJV license: The MIT License (MIT)

Copyright (c) 2015-2021 Evgeny Poberezkin

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

 */
'use strict';
export const validateToolCredentialRequest = validate52;
const schema19 = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  anyOf: [
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.outcome' },
        value: { $ref: '#/$defs/ToolCredentialOutcomeRequestV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.create' },
        value: { $ref: '#/$defs/ToolCredentialCreateRequestV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.rotate' },
        value: { $ref: '#/$defs/ToolCredentialChangeRequestV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.clear' },
        value: { $ref: '#/$defs/ToolCredentialChangeRequestV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.origins' },
        value: { $ref: '#/$defs/ToolCredentialOriginsRequestV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.recover' },
        value: { $ref: '#/$defs/ToolCredentialChangeRequestV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.list' },
        value: { $ref: '#/$defs/ToolCredentialManagementTargetV1' },
      },
    },
  ],
  $defs: {
    ToolCredentialOutcomeRequestV1: {
      additionalProperties: false,
      properties: {
        command_invocation_id: { $ref: '#/$defs/CommandInvocationId' },
        target: { $ref: '#/$defs/ToolCredentialManagementTargetV1' },
      },
      required: ['target', 'command_invocation_id'],
      type: 'object',
    },
    CommandInvocationId: { type: 'string' },
    ToolCredentialManagementTargetV1: {
      additionalProperties: false,
      properties: {
        credential_slot_key: { $ref: '#/$defs/CredentialSlotKey' },
        expected_instance_revision: { $ref: '#/$defs/Revision' },
        placement: { $ref: '#/$defs/ToolCredentialPlacementV1' },
        session_id: { anyOf: [{ $ref: '#/$defs/SessionId' }, { type: 'null' }] },
        tool_instance_id: { $ref: '#/$defs/ToolInstanceId' },
        workspace_id: { anyOf: [{ $ref: '#/$defs/WorkspaceId' }, { type: 'null' }] },
      },
      required: [
        'tool_instance_id',
        'expected_instance_revision',
        'credential_slot_key',
        'placement',
      ],
      type: 'object',
    },
    CredentialSlotKey: {
      maxLength: 64,
      minLength: 1,
      pattern: '^[a-z0-9]+(?:[-_][a-z0-9]+)*$',
      type: 'string',
    },
    Revision: { type: 'string' },
    ToolCredentialPlacementV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            environment_variable: { $ref: '#/$defs/EnvironmentVariableNameV1' },
            placement: { const: 'command_environment', type: 'string' },
          },
          required: ['placement', 'environment_variable'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: { placement: { const: 'http_bearer_authorization', type: 'string' } },
          required: ['placement'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            header_name: { $ref: '#/$defs/HttpHeaderNameV1' },
            placement: { const: 'http_fixed_header', type: 'string' },
          },
          required: ['placement', 'header_name'],
          type: 'object',
        },
      ],
    },
    EnvironmentVariableNameV1: {
      maxLength: 64,
      minLength: 1,
      not: {
        enum: [
          'PATH',
          'PATHEXT',
          'PSMODULEPATH',
          'COMSPEC',
          'SYSTEMROOT',
          'WINDIR',
          'TEMP',
          'TMP',
          'LOCALAPPDATA',
          'AURA_TOOL_CALL_ID',
          'AURA_TOOL_EXECUTION_ATTEMPT_ID',
          'AURA_TOOL_OUTPUT_ROOT',
        ],
      },
      pattern: '^[A-Z_][A-Z0-9_]{0,63}$',
      type: 'string',
    },
    HttpHeaderNameV1: {
      maxLength: 128,
      minLength: 1,
      not: {
        pattern:
          '^(?:[Hh][Oo][Ss][Tt]|[Cc][Oo][Nn][Tt][Ee][Nn][Tt]-[Ll][Ee][Nn][Gg][Tt][Hh]|[Tt][Rr][Aa][Nn][Ss][Ff][Ee][Rr]-[Ee][Nn][Cc][Oo][Dd][Ii][Nn][Gg]|[Cc][Oo][Nn][Nn][Ee][Cc][Tt][Ii][Oo][Nn]|[Aa][Uu][Tt][Hh][Oo][Rr][Ii][Zz][Aa][Tt][Ii][Oo][Nn]|[Cc][Oo][Oo][Kk][Ii][Ee]|[Ss][Ee][Tt]-[Cc][Oo][Oo][Kk][Ii][Ee]|[Uu][Pp][Gg][Rr][Aa][Dd][Ee]|[Tt][Rr][Aa][Ii][Ll][Ee][Rr]|[Ee][Xx][Pp][Ee][Cc][Tt]|[Tt][Ee]|[Pp][Rr][Oo][Xx][Yy]-.*)$',
      },
      pattern: "^[!#$%&'*+.^_`|~0-9A-Za-z-]+$",
      type: 'string',
    },
    SessionId: { type: 'string' },
    ToolInstanceId: { type: 'string' },
    WorkspaceId: { type: 'string' },
    ToolCredentialOutcomeV1: {
      additionalProperties: false,
      properties: {
        command_invocation_id: { $ref: '#/$defs/CommandInvocationId' },
        result: { anyOf: [{ $ref: '#/$defs/ToolCredentialMutationResultV1' }, { type: 'null' }] },
        state: { $ref: '#/$defs/ToolCredentialOutcomeStateV1' },
      },
      required: ['command_invocation_id', 'state'],
      type: 'object',
    },
    ToolCredentialMutationResultV1: {
      additionalProperties: false,
      properties: {
        cleanup_pending: {
          description:
            'The authoritative change committed; deletion of an old WinCred value may still need retry.',
          type: 'boolean',
        },
        credential: {
          anyOf: [{ $ref: '#/$defs/ToolCredentialAuthorityRecordV1' }, { type: 'null' }],
        },
        disposition: { $ref: '#/$defs/ToolCredentialMutationDispositionV1' },
      },
      required: ['disposition', 'cleanup_pending'],
      type: 'object',
    },
    ToolCredentialAuthorityRecordV1: {
      additionalProperties: false,
      description:
        'Authority metadata for a Tool Credential. Secret bytes and WinCred\nlocators are deliberately held by the P02 storage binding, never by this\nrenderer-safe domain record.',
      properties: {
        allowed_origins: {
          description:
            'User-configured HTTP origins. Empty historical records authorize no model-selected URL.',
          items: { type: 'string' },
          type: 'array',
        },
        authenticated_owner: { $ref: '#/$defs/PrincipalId' },
        created_at: { $ref: '#/$defs/UtcTimestamp' },
        credential_id: { $ref: '#/$defs/CredentialId' },
        credential_kind: { $ref: '#/$defs/ToolCredentialKindV1' },
        credential_revision: { $ref: '#/$defs/CredentialRevision' },
        display_name: { type: 'string' },
        placement_policy: { $ref: '#/$defs/ToolCredentialPlacementPolicyV1' },
        row_revision: { $ref: '#/$defs/Revision' },
        scope: { $ref: '#/$defs/ToolCredentialScopeV1' },
        state: { $ref: '#/$defs/ToolCredentialStateV1' },
        updated_at: { $ref: '#/$defs/UtcTimestamp' },
        usage_policy: { $ref: '#/$defs/ToolCredentialUsagePolicyV1' },
      },
      required: [
        'credential_id',
        'credential_revision',
        'authenticated_owner',
        'scope',
        'credential_kind',
        'state',
        'placement_policy',
        'usage_policy',
        'display_name',
        'row_revision',
        'created_at',
        'updated_at',
      ],
      type: 'object',
    },
    PrincipalId: { type: 'string' },
    UtcTimestamp: { type: 'string' },
    CredentialId: { type: 'string' },
    ToolCredentialKindV1: { enum: ['api_key'], type: 'string' },
    CredentialRevision: {
      $ref: '#/$defs/Revision',
      description:
        'Secret-generation revision for one stable CredentialId.\n\nThis is intentionally distinct from a row revision. Clear and metadata-only\nmutations may change row concurrency without creating a new secret\ngeneration.',
    },
    ToolCredentialPlacementPolicyV1: {
      additionalProperties: false,
      properties: {
        allowed_placements: { items: { $ref: '#/$defs/ToolCredentialPlacementV1' }, type: 'array' },
      },
      required: ['allowed_placements'],
      type: 'object',
    },
    ToolCredentialScopeV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: { scope: { const: 'user', type: 'string' } },
          required: ['scope'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            scope: { const: 'workspace', type: 'string' },
            workspace_id: { $ref: '#/$defs/WorkspaceId' },
          },
          required: ['scope', 'workspace_id'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            scope: { const: 'session', type: 'string' },
            session_id: { $ref: '#/$defs/SessionId' },
            workspace_id: { $ref: '#/$defs/WorkspaceId' },
          },
          required: ['scope', 'workspace_id', 'session_id'],
          type: 'object',
        },
      ],
    },
    ToolCredentialStateV1: {
      enum: ['configured', 'suspended', 'revoked', 'recovery_required'],
      type: 'string',
    },
    ToolCredentialUsagePolicyV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            policy: { const: 'explicit', type: 'string' },
            tool_ids: { items: { $ref: '#/$defs/ToolId' }, type: 'array' },
            tool_package_ids: { items: { $ref: '#/$defs/ToolPackageId' }, type: 'array' },
          },
          required: ['policy', 'tool_package_ids', 'tool_ids'],
          type: 'object',
        },
      ],
    },
    ToolId: { type: 'string' },
    ToolPackageId: { type: 'string' },
    ToolCredentialMutationDispositionV1: { enum: ['committed', 'aborted'], type: 'string' },
    ToolCredentialOutcomeStateV1: {
      enum: ['pending', 'committed', 'aborted', 'not_obtained'],
      type: 'string',
    },
    ToolCredentialCreateRequestV1: {
      additionalProperties: false,
      properties: {
        allowed_origins: { items: { type: 'string' }, type: 'array' },
        display_name: { type: 'string' },
        scope: { $ref: '#/$defs/ToolCredentialScopeV1' },
        target: { $ref: '#/$defs/ToolCredentialManagementTargetV1' },
      },
      required: ['target', 'display_name', 'scope'],
      type: 'object',
    },
    ToolCredentialChangeRequestV1: {
      additionalProperties: false,
      properties: {
        credential_id: { $ref: '#/$defs/CredentialId' },
        expected_row_revision: { $ref: '#/$defs/Revision' },
        target: { $ref: '#/$defs/ToolCredentialManagementTargetV1' },
      },
      required: ['target', 'credential_id', 'expected_row_revision'],
      type: 'object',
    },
    ToolCredentialOriginsRequestV1: {
      additionalProperties: false,
      properties: {
        allowed_origins: { items: { type: 'string' }, type: 'array' },
        credential_id: { $ref: '#/$defs/CredentialId' },
        expected_row_revision: { $ref: '#/$defs/Revision' },
        target: { $ref: '#/$defs/ToolCredentialManagementTargetV1' },
      },
      required: ['target', 'credential_id', 'expected_row_revision', 'allowed_origins'],
      type: 'object',
    },
    ToolCredentialListResultV1: {
      additionalProperties: false,
      properties: {
        credentials: { items: { $ref: '#/$defs/ToolCredentialAuthorityRecordV1' }, type: 'array' },
        pending_credential_ids: { items: { $ref: '#/$defs/CredentialId' }, type: 'array' },
      },
      required: ['credentials', 'pending_credential_ids'],
      type: 'object',
    },
  },
};
const schema20 = {
  additionalProperties: false,
  properties: {
    command_invocation_id: { $ref: '#/$defs/CommandInvocationId' },
    target: { $ref: '#/$defs/ToolCredentialManagementTargetV1' },
  },
  required: ['target', 'command_invocation_id'],
  type: 'object',
};
const schema21 = { type: 'string' };
function validate54(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate54.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate54.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate54.errors = vErrors;
  return errors === 0;
}
validate54.evaluated = { dynamicProps: false, dynamicItems: false };
const schema22 = {
  additionalProperties: false,
  properties: {
    credential_slot_key: { $ref: '#/$defs/CredentialSlotKey' },
    expected_instance_revision: { $ref: '#/$defs/Revision' },
    placement: { $ref: '#/$defs/ToolCredentialPlacementV1' },
    session_id: { anyOf: [{ $ref: '#/$defs/SessionId' }, { type: 'null' }] },
    tool_instance_id: { $ref: '#/$defs/ToolInstanceId' },
    workspace_id: { anyOf: [{ $ref: '#/$defs/WorkspaceId' }, { type: 'null' }] },
  },
  required: ['tool_instance_id', 'expected_instance_revision', 'credential_slot_key', 'placement'],
  type: 'object',
};
const schema23 = {
  maxLength: 64,
  minLength: 1,
  pattern: '^[a-z0-9]+(?:[-_][a-z0-9]+)*$',
  type: 'string',
};
const func1 = function ucs2length(str) {
  const len = str.length;
  let length = 0;
  let pos = 0;
  let value;
  while (pos < len) {
    length++;
    value = str.charCodeAt(pos++);
    if (value >= 0xd800 && value <= 0xdbff && pos < len) {
      // high surrogate, and there is a next character
      value = str.charCodeAt(pos);
      if ((value & 0xfc00) === 0xdc00) pos++; // low surrogate
    }
  }
  return length;
};
const pattern3 = new RegExp('^[a-z0-9]+(?:[-_][a-z0-9]+)*$', 'u');
function validate57(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate57.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 64) {
        validate57.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 64 },
            message: 'must NOT have more than 64 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 1) {
          validate57.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 1 },
              message: 'must NOT have fewer than 1 characters',
            },
          ];
          return false;
        } else {
          if (!pattern3.test(data)) {
            validate57.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: '^[a-z0-9]+(?:[-_][a-z0-9]+)*$' },
                message: 'must match pattern "' + '^[a-z0-9]+(?:[-_][a-z0-9]+)*$' + '"',
              },
            ];
            return false;
          }
        }
      }
    } else {
      validate57.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'string' },
          message: 'must be string',
        },
      ];
      return false;
    }
  }
  validate57.errors = vErrors;
  return errors === 0;
}
validate57.evaluated = { dynamicProps: false, dynamicItems: false };
const schema24 = { type: 'string' };
function validate59(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate59.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate59.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate59.errors = vErrors;
  return errors === 0;
}
validate59.evaluated = { dynamicProps: false, dynamicItems: false };
const schema25 = {
  oneOf: [
    {
      additionalProperties: false,
      properties: {
        environment_variable: { $ref: '#/$defs/EnvironmentVariableNameV1' },
        placement: { const: 'command_environment', type: 'string' },
      },
      required: ['placement', 'environment_variable'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: { placement: { const: 'http_bearer_authorization', type: 'string' } },
      required: ['placement'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: {
        header_name: { $ref: '#/$defs/HttpHeaderNameV1' },
        placement: { const: 'http_fixed_header', type: 'string' },
      },
      required: ['placement', 'header_name'],
      type: 'object',
    },
  ],
};
const schema26 = {
  maxLength: 64,
  minLength: 1,
  not: {
    enum: [
      'PATH',
      'PATHEXT',
      'PSMODULEPATH',
      'COMSPEC',
      'SYSTEMROOT',
      'WINDIR',
      'TEMP',
      'TMP',
      'LOCALAPPDATA',
      'AURA_TOOL_CALL_ID',
      'AURA_TOOL_EXECUTION_ATTEMPT_ID',
      'AURA_TOOL_OUTPUT_ROOT',
    ],
  },
  pattern: '^[A-Z_][A-Z0-9_]{0,63}$',
  type: 'string',
};
const pattern4 = new RegExp('^[A-Z_][A-Z0-9_]{0,63}$', 'u');
function validate62(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate62.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs1 = errors;
  const _errs2 = errors;
  if (!(
    data === 'PATH' ||
    data === 'PATHEXT' ||
    data === 'PSMODULEPATH' ||
    data === 'COMSPEC' ||
    data === 'SYSTEMROOT' ||
    data === 'WINDIR' ||
    data === 'TEMP' ||
    data === 'TMP' ||
    data === 'LOCALAPPDATA' ||
    data === 'AURA_TOOL_CALL_ID' ||
    data === 'AURA_TOOL_EXECUTION_ATTEMPT_ID' ||
    data === 'AURA_TOOL_OUTPUT_ROOT'
  )) {
    const err0 = {};
    if (vErrors === null) {
      vErrors = [err0];
    } else {
      vErrors.push(err0);
    }
    errors++;
  }
  var valid0 = _errs2 === errors;
  if (valid0) {
    validate62.errors = [
      {
        instancePath,
        schemaPath: '#/not',
        keyword: 'not',
        params: {},
        message: 'must NOT be valid',
      },
    ];
    return false;
  } else {
    errors = _errs1;
    if (vErrors !== null) {
      if (_errs1) {
        vErrors.length = _errs1;
      } else {
        vErrors = null;
      }
    }
  }
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 64) {
        validate62.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 64 },
            message: 'must NOT have more than 64 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 1) {
          validate62.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 1 },
              message: 'must NOT have fewer than 1 characters',
            },
          ];
          return false;
        } else {
          if (!pattern4.test(data)) {
            validate62.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: '^[A-Z_][A-Z0-9_]{0,63}$' },
                message: 'must match pattern "' + '^[A-Z_][A-Z0-9_]{0,63}$' + '"',
              },
            ];
            return false;
          }
        }
      }
    } else {
      validate62.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'string' },
          message: 'must be string',
        },
      ];
      return false;
    }
  }
  validate62.errors = vErrors;
  return errors === 0;
}
validate62.evaluated = { dynamicProps: false, dynamicItems: false };
const schema27 = {
  maxLength: 128,
  minLength: 1,
  not: {
    pattern:
      '^(?:[Hh][Oo][Ss][Tt]|[Cc][Oo][Nn][Tt][Ee][Nn][Tt]-[Ll][Ee][Nn][Gg][Tt][Hh]|[Tt][Rr][Aa][Nn][Ss][Ff][Ee][Rr]-[Ee][Nn][Cc][Oo][Dd][Ii][Nn][Gg]|[Cc][Oo][Nn][Nn][Ee][Cc][Tt][Ii][Oo][Nn]|[Aa][Uu][Tt][Hh][Oo][Rr][Ii][Zz][Aa][Tt][Ii][Oo][Nn]|[Cc][Oo][Oo][Kk][Ii][Ee]|[Ss][Ee][Tt]-[Cc][Oo][Oo][Kk][Ii][Ee]|[Uu][Pp][Gg][Rr][Aa][Dd][Ee]|[Tt][Rr][Aa][Ii][Ll][Ee][Rr]|[Ee][Xx][Pp][Ee][Cc][Tt]|[Tt][Ee]|[Pp][Rr][Oo][Xx][Yy]-.*)$',
  },
  pattern: "^[!#$%&'*+.^_`|~0-9A-Za-z-]+$",
  type: 'string',
};
const pattern5 = new RegExp(
  '^(?:[Hh][Oo][Ss][Tt]|[Cc][Oo][Nn][Tt][Ee][Nn][Tt]-[Ll][Ee][Nn][Gg][Tt][Hh]|[Tt][Rr][Aa][Nn][Ss][Ff][Ee][Rr]-[Ee][Nn][Cc][Oo][Dd][Ii][Nn][Gg]|[Cc][Oo][Nn][Nn][Ee][Cc][Tt][Ii][Oo][Nn]|[Aa][Uu][Tt][Hh][Oo][Rr][Ii][Zz][Aa][Tt][Ii][Oo][Nn]|[Cc][Oo][Oo][Kk][Ii][Ee]|[Ss][Ee][Tt]-[Cc][Oo][Oo][Kk][Ii][Ee]|[Uu][Pp][Gg][Rr][Aa][Dd][Ee]|[Tt][Rr][Aa][Ii][Ll][Ee][Rr]|[Ee][Xx][Pp][Ee][Cc][Tt]|[Tt][Ee]|[Pp][Rr][Oo][Xx][Yy]-.*)$',
  'u',
);
const pattern6 = new RegExp("^[!#$%&'*+.^_`|~0-9A-Za-z-]+$", 'u');
function validate64(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate64.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs1 = errors;
  const _errs2 = errors;
  if (typeof data === 'string') {
    if (!pattern5.test(data)) {
      const err0 = {};
      if (vErrors === null) {
        vErrors = [err0];
      } else {
        vErrors.push(err0);
      }
      errors++;
    }
  }
  var valid0 = _errs2 === errors;
  if (valid0) {
    validate64.errors = [
      {
        instancePath,
        schemaPath: '#/not',
        keyword: 'not',
        params: {},
        message: 'must NOT be valid',
      },
    ];
    return false;
  } else {
    errors = _errs1;
    if (vErrors !== null) {
      if (_errs1) {
        vErrors.length = _errs1;
      } else {
        vErrors = null;
      }
    }
  }
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 128) {
        validate64.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 128 },
            message: 'must NOT have more than 128 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 1) {
          validate64.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 1 },
              message: 'must NOT have fewer than 1 characters',
            },
          ];
          return false;
        } else {
          if (!pattern6.test(data)) {
            validate64.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: "^[!#$%&'*+.^_`|~0-9A-Za-z-]+$" },
                message: 'must match pattern "' + "^[!#$%&'*+.^_`|~0-9A-Za-z-]+$" + '"',
              },
            ];
            return false;
          }
        }
      }
    } else {
      validate64.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'string' },
          message: 'must be string',
        },
      ];
      return false;
    }
  }
  validate64.errors = vErrors;
  return errors === 0;
}
validate64.evaluated = { dynamicProps: false, dynamicItems: false };
function validate61(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate61.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs0 = errors;
  let valid0 = false;
  let passing0 = null;
  const _errs1 = errors;
  if (errors === _errs1) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.placement === undefined && (missing0 = 'placement')) ||
        (data.environment_variable === undefined && (missing0 = 'environment_variable'))
      ) {
        const err0 = {
          instancePath,
          schemaPath: '#/oneOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs3 = errors;
        for (const key0 in data) {
          if (!(key0 === 'environment_variable' || key0 === 'placement')) {
            const err1 = {
              instancePath,
              schemaPath: '#/oneOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs3 === errors) {
          if (data.environment_variable !== undefined) {
            const _errs4 = errors;
            if (
              !validate62(data.environment_variable, {
                instancePath: instancePath + '/environment_variable',
                parentData: data,
                parentDataProperty: 'environment_variable',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate62.errors : vErrors.concat(validate62.errors);
              errors = vErrors.length;
            }
            var valid1 = _errs4 === errors;
          } else {
            var valid1 = true;
          }
          if (valid1) {
            if (data.placement !== undefined) {
              let data1 = data.placement;
              const _errs5 = errors;
              if (typeof data1 !== 'string') {
                const err2 = {
                  instancePath: instancePath + '/placement',
                  schemaPath: '#/oneOf/0/properties/placement/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                };
                if (vErrors === null) {
                  vErrors = [err2];
                } else {
                  vErrors.push(err2);
                }
                errors++;
              }
              if ('command_environment' !== data1) {
                const err3 = {
                  instancePath: instancePath + '/placement',
                  schemaPath: '#/oneOf/0/properties/placement/const',
                  keyword: 'const',
                  params: { allowedValue: 'command_environment' },
                  message: 'must be equal to constant',
                };
                if (vErrors === null) {
                  vErrors = [err3];
                } else {
                  vErrors.push(err3);
                }
                errors++;
              }
              var valid1 = _errs5 === errors;
            } else {
              var valid1 = true;
            }
          }
        }
      }
    } else {
      const err4 = {
        instancePath,
        schemaPath: '#/oneOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err4];
      } else {
        vErrors.push(err4);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  if (_valid0) {
    valid0 = true;
    passing0 = 0;
    var props0 = true;
  }
  const _errs7 = errors;
  if (errors === _errs7) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (data.placement === undefined && (missing1 = 'placement')) {
        const err5 = {
          instancePath,
          schemaPath: '#/oneOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err5];
        } else {
          vErrors.push(err5);
        }
        errors++;
      } else {
        const _errs9 = errors;
        for (const key1 in data) {
          if (!(key1 === 'placement')) {
            const err6 = {
              instancePath,
              schemaPath: '#/oneOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err6];
            } else {
              vErrors.push(err6);
            }
            errors++;
            break;
          }
        }
        if (_errs9 === errors) {
          if (data.placement !== undefined) {
            let data2 = data.placement;
            if (typeof data2 !== 'string') {
              const err7 = {
                instancePath: instancePath + '/placement',
                schemaPath: '#/oneOf/1/properties/placement/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err7];
              } else {
                vErrors.push(err7);
              }
              errors++;
            }
            if ('http_bearer_authorization' !== data2) {
              const err8 = {
                instancePath: instancePath + '/placement',
                schemaPath: '#/oneOf/1/properties/placement/const',
                keyword: 'const',
                params: { allowedValue: 'http_bearer_authorization' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err8];
              } else {
                vErrors.push(err8);
              }
              errors++;
            }
          }
        }
      }
    } else {
      const err9 = {
        instancePath,
        schemaPath: '#/oneOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err9];
      } else {
        vErrors.push(err9);
      }
      errors++;
    }
  }
  var _valid0 = _errs7 === errors;
  if (_valid0 && valid0) {
    valid0 = false;
    passing0 = [passing0, 1];
  } else {
    if (_valid0) {
      valid0 = true;
      passing0 = 1;
      if (props0 !== true) {
        props0 = true;
      }
    }
    const _errs12 = errors;
    if (errors === _errs12) {
      if (data && typeof data == 'object' && !Array.isArray(data)) {
        let missing2;
        if (
          (data.placement === undefined && (missing2 = 'placement')) ||
          (data.header_name === undefined && (missing2 = 'header_name'))
        ) {
          const err10 = {
            instancePath,
            schemaPath: '#/oneOf/2/required',
            keyword: 'required',
            params: { missingProperty: missing2 },
            message: "must have required property '" + missing2 + "'",
          };
          if (vErrors === null) {
            vErrors = [err10];
          } else {
            vErrors.push(err10);
          }
          errors++;
        } else {
          const _errs14 = errors;
          for (const key2 in data) {
            if (!(key2 === 'header_name' || key2 === 'placement')) {
              const err11 = {
                instancePath,
                schemaPath: '#/oneOf/2/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key2 },
                message: 'must NOT have additional properties',
              };
              if (vErrors === null) {
                vErrors = [err11];
              } else {
                vErrors.push(err11);
              }
              errors++;
              break;
            }
          }
          if (_errs14 === errors) {
            if (data.header_name !== undefined) {
              const _errs15 = errors;
              if (
                !validate64(data.header_name, {
                  instancePath: instancePath + '/header_name',
                  parentData: data,
                  parentDataProperty: 'header_name',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate64.errors : vErrors.concat(validate64.errors);
                errors = vErrors.length;
              }
              var valid3 = _errs15 === errors;
            } else {
              var valid3 = true;
            }
            if (valid3) {
              if (data.placement !== undefined) {
                let data4 = data.placement;
                const _errs16 = errors;
                if (typeof data4 !== 'string') {
                  const err12 = {
                    instancePath: instancePath + '/placement',
                    schemaPath: '#/oneOf/2/properties/placement/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  };
                  if (vErrors === null) {
                    vErrors = [err12];
                  } else {
                    vErrors.push(err12);
                  }
                  errors++;
                }
                if ('http_fixed_header' !== data4) {
                  const err13 = {
                    instancePath: instancePath + '/placement',
                    schemaPath: '#/oneOf/2/properties/placement/const',
                    keyword: 'const',
                    params: { allowedValue: 'http_fixed_header' },
                    message: 'must be equal to constant',
                  };
                  if (vErrors === null) {
                    vErrors = [err13];
                  } else {
                    vErrors.push(err13);
                  }
                  errors++;
                }
                var valid3 = _errs16 === errors;
              } else {
                var valid3 = true;
              }
            }
          }
        }
      } else {
        const err14 = {
          instancePath,
          schemaPath: '#/oneOf/2/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        };
        if (vErrors === null) {
          vErrors = [err14];
        } else {
          vErrors.push(err14);
        }
        errors++;
      }
    }
    var _valid0 = _errs12 === errors;
    if (_valid0 && valid0) {
      valid0 = false;
      passing0 = [passing0, 2];
    } else {
      if (_valid0) {
        valid0 = true;
        passing0 = 2;
        if (props0 !== true) {
          props0 = true;
        }
      }
    }
  }
  if (!valid0) {
    const err15 = {
      instancePath,
      schemaPath: '#/oneOf',
      keyword: 'oneOf',
      params: { passingSchemas: passing0 },
      message: 'must match exactly one schema in oneOf',
    };
    if (vErrors === null) {
      vErrors = [err15];
    } else {
      vErrors.push(err15);
    }
    errors++;
    validate61.errors = vErrors;
    return false;
  } else {
    errors = _errs0;
    if (vErrors !== null) {
      if (_errs0) {
        vErrors.length = _errs0;
      } else {
        vErrors = null;
      }
    }
  }
  validate61.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate61.evaluated = { dynamicProps: true, dynamicItems: false };
const schema28 = { type: 'string' };
function validate67(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate67.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate67.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate67.errors = vErrors;
  return errors === 0;
}
validate67.evaluated = { dynamicProps: false, dynamicItems: false };
const schema29 = { type: 'string' };
function validate69(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate69.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate69.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate69.errors = vErrors;
  return errors === 0;
}
validate69.evaluated = { dynamicProps: false, dynamicItems: false };
const schema30 = { type: 'string' };
function validate71(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate71.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate71.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate71.errors = vErrors;
  return errors === 0;
}
validate71.evaluated = { dynamicProps: false, dynamicItems: false };
function validate56(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate56.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.tool_instance_id === undefined && (missing0 = 'tool_instance_id')) ||
        (data.expected_instance_revision === undefined &&
          (missing0 = 'expected_instance_revision')) ||
        (data.credential_slot_key === undefined && (missing0 = 'credential_slot_key')) ||
        (data.placement === undefined && (missing0 = 'placement'))
      ) {
        validate56.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'credential_slot_key' ||
            key0 === 'expected_instance_revision' ||
            key0 === 'placement' ||
            key0 === 'session_id' ||
            key0 === 'tool_instance_id' ||
            key0 === 'workspace_id'
          )) {
            validate56.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.credential_slot_key !== undefined) {
            const _errs2 = errors;
            if (
              !validate57(data.credential_slot_key, {
                instancePath: instancePath + '/credential_slot_key',
                parentData: data,
                parentDataProperty: 'credential_slot_key',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate57.errors : vErrors.concat(validate57.errors);
              errors = vErrors.length;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.expected_instance_revision !== undefined) {
              const _errs3 = errors;
              if (
                !validate59(data.expected_instance_revision, {
                  instancePath: instancePath + '/expected_instance_revision',
                  parentData: data,
                  parentDataProperty: 'expected_instance_revision',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate59.errors : vErrors.concat(validate59.errors);
                errors = vErrors.length;
              }
              var valid0 = _errs3 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.placement !== undefined) {
                const _errs4 = errors;
                if (
                  !validate61(data.placement, {
                    instancePath: instancePath + '/placement',
                    parentData: data,
                    parentDataProperty: 'placement',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate61.errors : vErrors.concat(validate61.errors);
                  errors = vErrors.length;
                }
                var valid0 = _errs4 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.session_id !== undefined) {
                  let data3 = data.session_id;
                  const _errs5 = errors;
                  const _errs6 = errors;
                  let valid1 = false;
                  const _errs7 = errors;
                  if (
                    !validate67(data3, {
                      instancePath: instancePath + '/session_id',
                      parentData: data,
                      parentDataProperty: 'session_id',
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate67.errors : vErrors.concat(validate67.errors);
                    errors = vErrors.length;
                  }
                  var _valid0 = _errs7 === errors;
                  valid1 = valid1 || _valid0;
                  const _errs8 = errors;
                  if (data3 !== null) {
                    const err0 = {
                      instancePath: instancePath + '/session_id',
                      schemaPath: '#/properties/session_id/anyOf/1/type',
                      keyword: 'type',
                      params: { type: 'null' },
                      message: 'must be null',
                    };
                    if (vErrors === null) {
                      vErrors = [err0];
                    } else {
                      vErrors.push(err0);
                    }
                    errors++;
                  }
                  var _valid0 = _errs8 === errors;
                  valid1 = valid1 || _valid0;
                  if (!valid1) {
                    const err1 = {
                      instancePath: instancePath + '/session_id',
                      schemaPath: '#/properties/session_id/anyOf',
                      keyword: 'anyOf',
                      params: {},
                      message: 'must match a schema in anyOf',
                    };
                    if (vErrors === null) {
                      vErrors = [err1];
                    } else {
                      vErrors.push(err1);
                    }
                    errors++;
                    validate56.errors = vErrors;
                    return false;
                  } else {
                    errors = _errs6;
                    if (vErrors !== null) {
                      if (_errs6) {
                        vErrors.length = _errs6;
                      } else {
                        vErrors = null;
                      }
                    }
                  }
                  var valid0 = _errs5 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.tool_instance_id !== undefined) {
                    const _errs10 = errors;
                    if (
                      !validate69(data.tool_instance_id, {
                        instancePath: instancePath + '/tool_instance_id',
                        parentData: data,
                        parentDataProperty: 'tool_instance_id',
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate69.errors : vErrors.concat(validate69.errors);
                      errors = vErrors.length;
                    }
                    var valid0 = _errs10 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.workspace_id !== undefined) {
                      let data5 = data.workspace_id;
                      const _errs11 = errors;
                      const _errs12 = errors;
                      let valid2 = false;
                      const _errs13 = errors;
                      if (
                        !validate71(data5, {
                          instancePath: instancePath + '/workspace_id',
                          parentData: data,
                          parentDataProperty: 'workspace_id',
                          rootData,
                          dynamicAnchors,
                        })
                      ) {
                        vErrors =
                          vErrors === null ? validate71.errors : vErrors.concat(validate71.errors);
                        errors = vErrors.length;
                      }
                      var _valid1 = _errs13 === errors;
                      valid2 = valid2 || _valid1;
                      const _errs14 = errors;
                      if (data5 !== null) {
                        const err2 = {
                          instancePath: instancePath + '/workspace_id',
                          schemaPath: '#/properties/workspace_id/anyOf/1/type',
                          keyword: 'type',
                          params: { type: 'null' },
                          message: 'must be null',
                        };
                        if (vErrors === null) {
                          vErrors = [err2];
                        } else {
                          vErrors.push(err2);
                        }
                        errors++;
                      }
                      var _valid1 = _errs14 === errors;
                      valid2 = valid2 || _valid1;
                      if (!valid2) {
                        const err3 = {
                          instancePath: instancePath + '/workspace_id',
                          schemaPath: '#/properties/workspace_id/anyOf',
                          keyword: 'anyOf',
                          params: {},
                          message: 'must match a schema in anyOf',
                        };
                        if (vErrors === null) {
                          vErrors = [err3];
                        } else {
                          vErrors.push(err3);
                        }
                        errors++;
                        validate56.errors = vErrors;
                        return false;
                      } else {
                        errors = _errs12;
                        if (vErrors !== null) {
                          if (_errs12) {
                            vErrors.length = _errs12;
                          } else {
                            vErrors = null;
                          }
                        }
                      }
                      var valid0 = _errs11 === errors;
                    } else {
                      var valid0 = true;
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate56.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate56.errors = vErrors;
  return errors === 0;
}
validate56.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate53(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate53.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.target === undefined && (missing0 = 'target')) ||
        (data.command_invocation_id === undefined && (missing0 = 'command_invocation_id'))
      ) {
        validate53.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(key0 === 'command_invocation_id' || key0 === 'target')) {
            validate53.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.command_invocation_id !== undefined) {
            const _errs2 = errors;
            if (
              !validate54(data.command_invocation_id, {
                instancePath: instancePath + '/command_invocation_id',
                parentData: data,
                parentDataProperty: 'command_invocation_id',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate54.errors : vErrors.concat(validate54.errors);
              errors = vErrors.length;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.target !== undefined) {
              const _errs3 = errors;
              if (
                !validate56(data.target, {
                  instancePath: instancePath + '/target',
                  parentData: data,
                  parentDataProperty: 'target',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate56.errors : vErrors.concat(validate56.errors);
                errors = vErrors.length;
              }
              var valid0 = _errs3 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate53.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate53.errors = vErrors;
  return errors === 0;
}
validate53.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema31 = {
  additionalProperties: false,
  properties: {
    allowed_origins: { items: { type: 'string' }, type: 'array' },
    display_name: { type: 'string' },
    scope: { $ref: '#/$defs/ToolCredentialScopeV1' },
    target: { $ref: '#/$defs/ToolCredentialManagementTargetV1' },
  },
  required: ['target', 'display_name', 'scope'],
  type: 'object',
};
const schema32 = {
  oneOf: [
    {
      additionalProperties: false,
      properties: { scope: { const: 'user', type: 'string' } },
      required: ['scope'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: {
        scope: { const: 'workspace', type: 'string' },
        workspace_id: { $ref: '#/$defs/WorkspaceId' },
      },
      required: ['scope', 'workspace_id'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: {
        scope: { const: 'session', type: 'string' },
        session_id: { $ref: '#/$defs/SessionId' },
        workspace_id: { $ref: '#/$defs/WorkspaceId' },
      },
      required: ['scope', 'workspace_id', 'session_id'],
      type: 'object',
    },
  ],
};
function validate76(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate76.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs0 = errors;
  let valid0 = false;
  let passing0 = null;
  const _errs1 = errors;
  if (errors === _errs1) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (data.scope === undefined && (missing0 = 'scope')) {
        const err0 = {
          instancePath,
          schemaPath: '#/oneOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs3 = errors;
        for (const key0 in data) {
          if (!(key0 === 'scope')) {
            const err1 = {
              instancePath,
              schemaPath: '#/oneOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs3 === errors) {
          if (data.scope !== undefined) {
            let data0 = data.scope;
            if (typeof data0 !== 'string') {
              const err2 = {
                instancePath: instancePath + '/scope',
                schemaPath: '#/oneOf/0/properties/scope/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err2];
              } else {
                vErrors.push(err2);
              }
              errors++;
            }
            if ('user' !== data0) {
              const err3 = {
                instancePath: instancePath + '/scope',
                schemaPath: '#/oneOf/0/properties/scope/const',
                keyword: 'const',
                params: { allowedValue: 'user' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err3];
              } else {
                vErrors.push(err3);
              }
              errors++;
            }
          }
        }
      }
    } else {
      const err4 = {
        instancePath,
        schemaPath: '#/oneOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err4];
      } else {
        vErrors.push(err4);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  if (_valid0) {
    valid0 = true;
    passing0 = 0;
    var props0 = true;
  }
  const _errs6 = errors;
  if (errors === _errs6) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (
        (data.scope === undefined && (missing1 = 'scope')) ||
        (data.workspace_id === undefined && (missing1 = 'workspace_id'))
      ) {
        const err5 = {
          instancePath,
          schemaPath: '#/oneOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err5];
        } else {
          vErrors.push(err5);
        }
        errors++;
      } else {
        const _errs8 = errors;
        for (const key1 in data) {
          if (!(key1 === 'scope' || key1 === 'workspace_id')) {
            const err6 = {
              instancePath,
              schemaPath: '#/oneOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err6];
            } else {
              vErrors.push(err6);
            }
            errors++;
            break;
          }
        }
        if (_errs8 === errors) {
          if (data.scope !== undefined) {
            let data1 = data.scope;
            const _errs9 = errors;
            if (typeof data1 !== 'string') {
              const err7 = {
                instancePath: instancePath + '/scope',
                schemaPath: '#/oneOf/1/properties/scope/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err7];
              } else {
                vErrors.push(err7);
              }
              errors++;
            }
            if ('workspace' !== data1) {
              const err8 = {
                instancePath: instancePath + '/scope',
                schemaPath: '#/oneOf/1/properties/scope/const',
                keyword: 'const',
                params: { allowedValue: 'workspace' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err8];
              } else {
                vErrors.push(err8);
              }
              errors++;
            }
            var valid2 = _errs9 === errors;
          } else {
            var valid2 = true;
          }
          if (valid2) {
            if (data.workspace_id !== undefined) {
              const _errs11 = errors;
              if (
                !validate71(data.workspace_id, {
                  instancePath: instancePath + '/workspace_id',
                  parentData: data,
                  parentDataProperty: 'workspace_id',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate71.errors : vErrors.concat(validate71.errors);
                errors = vErrors.length;
              }
              var valid2 = _errs11 === errors;
            } else {
              var valid2 = true;
            }
          }
        }
      }
    } else {
      const err9 = {
        instancePath,
        schemaPath: '#/oneOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err9];
      } else {
        vErrors.push(err9);
      }
      errors++;
    }
  }
  var _valid0 = _errs6 === errors;
  if (_valid0 && valid0) {
    valid0 = false;
    passing0 = [passing0, 1];
  } else {
    if (_valid0) {
      valid0 = true;
      passing0 = 1;
      if (props0 !== true) {
        props0 = true;
      }
    }
    const _errs12 = errors;
    if (errors === _errs12) {
      if (data && typeof data == 'object' && !Array.isArray(data)) {
        let missing2;
        if (
          (data.scope === undefined && (missing2 = 'scope')) ||
          (data.workspace_id === undefined && (missing2 = 'workspace_id')) ||
          (data.session_id === undefined && (missing2 = 'session_id'))
        ) {
          const err10 = {
            instancePath,
            schemaPath: '#/oneOf/2/required',
            keyword: 'required',
            params: { missingProperty: missing2 },
            message: "must have required property '" + missing2 + "'",
          };
          if (vErrors === null) {
            vErrors = [err10];
          } else {
            vErrors.push(err10);
          }
          errors++;
        } else {
          const _errs14 = errors;
          for (const key2 in data) {
            if (!(key2 === 'scope' || key2 === 'session_id' || key2 === 'workspace_id')) {
              const err11 = {
                instancePath,
                schemaPath: '#/oneOf/2/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key2 },
                message: 'must NOT have additional properties',
              };
              if (vErrors === null) {
                vErrors = [err11];
              } else {
                vErrors.push(err11);
              }
              errors++;
              break;
            }
          }
          if (_errs14 === errors) {
            if (data.scope !== undefined) {
              let data3 = data.scope;
              const _errs15 = errors;
              if (typeof data3 !== 'string') {
                const err12 = {
                  instancePath: instancePath + '/scope',
                  schemaPath: '#/oneOf/2/properties/scope/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                };
                if (vErrors === null) {
                  vErrors = [err12];
                } else {
                  vErrors.push(err12);
                }
                errors++;
              }
              if ('session' !== data3) {
                const err13 = {
                  instancePath: instancePath + '/scope',
                  schemaPath: '#/oneOf/2/properties/scope/const',
                  keyword: 'const',
                  params: { allowedValue: 'session' },
                  message: 'must be equal to constant',
                };
                if (vErrors === null) {
                  vErrors = [err13];
                } else {
                  vErrors.push(err13);
                }
                errors++;
              }
              var valid3 = _errs15 === errors;
            } else {
              var valid3 = true;
            }
            if (valid3) {
              if (data.session_id !== undefined) {
                const _errs17 = errors;
                if (
                  !validate67(data.session_id, {
                    instancePath: instancePath + '/session_id',
                    parentData: data,
                    parentDataProperty: 'session_id',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate67.errors : vErrors.concat(validate67.errors);
                  errors = vErrors.length;
                }
                var valid3 = _errs17 === errors;
              } else {
                var valid3 = true;
              }
              if (valid3) {
                if (data.workspace_id !== undefined) {
                  const _errs18 = errors;
                  if (
                    !validate71(data.workspace_id, {
                      instancePath: instancePath + '/workspace_id',
                      parentData: data,
                      parentDataProperty: 'workspace_id',
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate71.errors : vErrors.concat(validate71.errors);
                    errors = vErrors.length;
                  }
                  var valid3 = _errs18 === errors;
                } else {
                  var valid3 = true;
                }
              }
            }
          }
        }
      } else {
        const err14 = {
          instancePath,
          schemaPath: '#/oneOf/2/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        };
        if (vErrors === null) {
          vErrors = [err14];
        } else {
          vErrors.push(err14);
        }
        errors++;
      }
    }
    var _valid0 = _errs12 === errors;
    if (_valid0 && valid0) {
      valid0 = false;
      passing0 = [passing0, 2];
    } else {
      if (_valid0) {
        valid0 = true;
        passing0 = 2;
        if (props0 !== true) {
          props0 = true;
        }
      }
    }
  }
  if (!valid0) {
    const err15 = {
      instancePath,
      schemaPath: '#/oneOf',
      keyword: 'oneOf',
      params: { passingSchemas: passing0 },
      message: 'must match exactly one schema in oneOf',
    };
    if (vErrors === null) {
      vErrors = [err15];
    } else {
      vErrors.push(err15);
    }
    errors++;
    validate76.errors = vErrors;
    return false;
  } else {
    errors = _errs0;
    if (vErrors !== null) {
      if (_errs0) {
        vErrors.length = _errs0;
      } else {
        vErrors = null;
      }
    }
  }
  validate76.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate76.evaluated = { dynamicProps: true, dynamicItems: false };
function validate75(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate75.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.target === undefined && (missing0 = 'target')) ||
        (data.display_name === undefined && (missing0 = 'display_name')) ||
        (data.scope === undefined && (missing0 = 'scope'))
      ) {
        validate75.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'allowed_origins' ||
            key0 === 'display_name' ||
            key0 === 'scope' ||
            key0 === 'target'
          )) {
            validate75.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.allowed_origins !== undefined) {
            let data0 = data.allowed_origins;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  const _errs4 = errors;
                  if (typeof data0[i0] !== 'string') {
                    validate75.errors = [
                      {
                        instancePath: instancePath + '/allowed_origins/' + i0,
                        schemaPath: '#/properties/allowed_origins/items/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      },
                    ];
                    return false;
                  }
                  var valid1 = _errs4 === errors;
                  if (!valid1) {
                    break;
                  }
                }
              } else {
                validate75.errors = [
                  {
                    instancePath: instancePath + '/allowed_origins',
                    schemaPath: '#/properties/allowed_origins/type',
                    keyword: 'type',
                    params: { type: 'array' },
                    message: 'must be array',
                  },
                ];
                return false;
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.display_name !== undefined) {
              const _errs6 = errors;
              if (typeof data.display_name !== 'string') {
                validate75.errors = [
                  {
                    instancePath: instancePath + '/display_name',
                    schemaPath: '#/properties/display_name/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  },
                ];
                return false;
              }
              var valid0 = _errs6 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.scope !== undefined) {
                const _errs8 = errors;
                if (
                  !validate76(data.scope, {
                    instancePath: instancePath + '/scope',
                    parentData: data,
                    parentDataProperty: 'scope',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate76.errors : vErrors.concat(validate76.errors);
                  errors = vErrors.length;
                }
                var valid0 = _errs8 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.target !== undefined) {
                  const _errs9 = errors;
                  if (
                    !validate56(data.target, {
                      instancePath: instancePath + '/target',
                      parentData: data,
                      parentDataProperty: 'target',
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate56.errors : vErrors.concat(validate56.errors);
                    errors = vErrors.length;
                  }
                  var valid0 = _errs9 === errors;
                } else {
                  var valid0 = true;
                }
              }
            }
          }
        }
      }
    } else {
      validate75.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate75.errors = vErrors;
  return errors === 0;
}
validate75.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema33 = {
  additionalProperties: false,
  properties: {
    credential_id: { $ref: '#/$defs/CredentialId' },
    expected_row_revision: { $ref: '#/$defs/Revision' },
    target: { $ref: '#/$defs/ToolCredentialManagementTargetV1' },
  },
  required: ['target', 'credential_id', 'expected_row_revision'],
  type: 'object',
};
const schema34 = { type: 'string' };
function validate84(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate84.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate84.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate84.errors = vErrors;
  return errors === 0;
}
validate84.evaluated = { dynamicProps: false, dynamicItems: false };
function validate83(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate83.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.target === undefined && (missing0 = 'target')) ||
        (data.credential_id === undefined && (missing0 = 'credential_id')) ||
        (data.expected_row_revision === undefined && (missing0 = 'expected_row_revision'))
      ) {
        validate83.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'credential_id' ||
            key0 === 'expected_row_revision' ||
            key0 === 'target'
          )) {
            validate83.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.credential_id !== undefined) {
            const _errs2 = errors;
            if (
              !validate84(data.credential_id, {
                instancePath: instancePath + '/credential_id',
                parentData: data,
                parentDataProperty: 'credential_id',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate84.errors : vErrors.concat(validate84.errors);
              errors = vErrors.length;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.expected_row_revision !== undefined) {
              const _errs3 = errors;
              if (
                !validate59(data.expected_row_revision, {
                  instancePath: instancePath + '/expected_row_revision',
                  parentData: data,
                  parentDataProperty: 'expected_row_revision',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate59.errors : vErrors.concat(validate59.errors);
                errors = vErrors.length;
              }
              var valid0 = _errs3 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.target !== undefined) {
                const _errs4 = errors;
                if (
                  !validate56(data.target, {
                    instancePath: instancePath + '/target',
                    parentData: data,
                    parentDataProperty: 'target',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate56.errors : vErrors.concat(validate56.errors);
                  errors = vErrors.length;
                }
                var valid0 = _errs4 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate83.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate83.errors = vErrors;
  return errors === 0;
}
validate83.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema35 = {
  additionalProperties: false,
  properties: {
    allowed_origins: { items: { type: 'string' }, type: 'array' },
    credential_id: { $ref: '#/$defs/CredentialId' },
    expected_row_revision: { $ref: '#/$defs/Revision' },
    target: { $ref: '#/$defs/ToolCredentialManagementTargetV1' },
  },
  required: ['target', 'credential_id', 'expected_row_revision', 'allowed_origins'],
  type: 'object',
};
function validate90(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate90.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.target === undefined && (missing0 = 'target')) ||
        (data.credential_id === undefined && (missing0 = 'credential_id')) ||
        (data.expected_row_revision === undefined && (missing0 = 'expected_row_revision')) ||
        (data.allowed_origins === undefined && (missing0 = 'allowed_origins'))
      ) {
        validate90.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'allowed_origins' ||
            key0 === 'credential_id' ||
            key0 === 'expected_row_revision' ||
            key0 === 'target'
          )) {
            validate90.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.allowed_origins !== undefined) {
            let data0 = data.allowed_origins;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  const _errs4 = errors;
                  if (typeof data0[i0] !== 'string') {
                    validate90.errors = [
                      {
                        instancePath: instancePath + '/allowed_origins/' + i0,
                        schemaPath: '#/properties/allowed_origins/items/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      },
                    ];
                    return false;
                  }
                  var valid1 = _errs4 === errors;
                  if (!valid1) {
                    break;
                  }
                }
              } else {
                validate90.errors = [
                  {
                    instancePath: instancePath + '/allowed_origins',
                    schemaPath: '#/properties/allowed_origins/type',
                    keyword: 'type',
                    params: { type: 'array' },
                    message: 'must be array',
                  },
                ];
                return false;
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.credential_id !== undefined) {
              const _errs6 = errors;
              if (
                !validate84(data.credential_id, {
                  instancePath: instancePath + '/credential_id',
                  parentData: data,
                  parentDataProperty: 'credential_id',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate84.errors : vErrors.concat(validate84.errors);
                errors = vErrors.length;
              }
              var valid0 = _errs6 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.expected_row_revision !== undefined) {
                const _errs7 = errors;
                if (
                  !validate59(data.expected_row_revision, {
                    instancePath: instancePath + '/expected_row_revision',
                    parentData: data,
                    parentDataProperty: 'expected_row_revision',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate59.errors : vErrors.concat(validate59.errors);
                  errors = vErrors.length;
                }
                var valid0 = _errs7 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.target !== undefined) {
                  const _errs8 = errors;
                  if (
                    !validate56(data.target, {
                      instancePath: instancePath + '/target',
                      parentData: data,
                      parentDataProperty: 'target',
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate56.errors : vErrors.concat(validate56.errors);
                    errors = vErrors.length;
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
              }
            }
          }
        }
      }
    } else {
      validate90.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate90.errors = vErrors;
  return errors === 0;
}
validate90.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate52(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate52.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs0 = errors;
  let valid0 = false;
  const _errs1 = errors;
  if (errors === _errs1) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.command === undefined && (missing0 = 'command')) ||
        (data.value === undefined && (missing0 = 'value'))
      ) {
        const err0 = {
          instancePath,
          schemaPath: '#/anyOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs3 = errors;
        for (const key0 in data) {
          if (!(key0 === 'command' || key0 === 'value')) {
            const err1 = {
              instancePath,
              schemaPath: '#/anyOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs3 === errors) {
          if (data.command !== undefined) {
            const _errs4 = errors;
            if ('tools.credential.outcome' !== data.command) {
              const err2 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/0/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.outcome' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err2];
              } else {
                vErrors.push(err2);
              }
              errors++;
            }
            var valid1 = _errs4 === errors;
          } else {
            var valid1 = true;
          }
          if (valid1) {
            if (data.value !== undefined) {
              const _errs5 = errors;
              if (
                !validate53(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate53.errors : vErrors.concat(validate53.errors);
                errors = vErrors.length;
              }
              var valid1 = _errs5 === errors;
            } else {
              var valid1 = true;
            }
          }
        }
      }
    } else {
      const err3 = {
        instancePath,
        schemaPath: '#/anyOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err3];
      } else {
        vErrors.push(err3);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    var props0 = true;
  }
  const _errs6 = errors;
  if (errors === _errs6) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (
        (data.command === undefined && (missing1 = 'command')) ||
        (data.value === undefined && (missing1 = 'value'))
      ) {
        const err4 = {
          instancePath,
          schemaPath: '#/anyOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err4];
        } else {
          vErrors.push(err4);
        }
        errors++;
      } else {
        const _errs8 = errors;
        for (const key1 in data) {
          if (!(key1 === 'command' || key1 === 'value')) {
            const err5 = {
              instancePath,
              schemaPath: '#/anyOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err5];
            } else {
              vErrors.push(err5);
            }
            errors++;
            break;
          }
        }
        if (_errs8 === errors) {
          if (data.command !== undefined) {
            const _errs9 = errors;
            if ('tools.credential.create' !== data.command) {
              const err6 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/1/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.create' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err6];
              } else {
                vErrors.push(err6);
              }
              errors++;
            }
            var valid2 = _errs9 === errors;
          } else {
            var valid2 = true;
          }
          if (valid2) {
            if (data.value !== undefined) {
              const _errs10 = errors;
              if (
                !validate75(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate75.errors : vErrors.concat(validate75.errors);
                errors = vErrors.length;
              }
              var valid2 = _errs10 === errors;
            } else {
              var valid2 = true;
            }
          }
        }
      }
    } else {
      const err7 = {
        instancePath,
        schemaPath: '#/anyOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err7];
      } else {
        vErrors.push(err7);
      }
      errors++;
    }
  }
  var _valid0 = _errs6 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs11 = errors;
  if (errors === _errs11) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing2;
      if (
        (data.command === undefined && (missing2 = 'command')) ||
        (data.value === undefined && (missing2 = 'value'))
      ) {
        const err8 = {
          instancePath,
          schemaPath: '#/anyOf/2/required',
          keyword: 'required',
          params: { missingProperty: missing2 },
          message: "must have required property '" + missing2 + "'",
        };
        if (vErrors === null) {
          vErrors = [err8];
        } else {
          vErrors.push(err8);
        }
        errors++;
      } else {
        const _errs13 = errors;
        for (const key2 in data) {
          if (!(key2 === 'command' || key2 === 'value')) {
            const err9 = {
              instancePath,
              schemaPath: '#/anyOf/2/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key2 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err9];
            } else {
              vErrors.push(err9);
            }
            errors++;
            break;
          }
        }
        if (_errs13 === errors) {
          if (data.command !== undefined) {
            const _errs14 = errors;
            if ('tools.credential.rotate' !== data.command) {
              const err10 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/2/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.rotate' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err10];
              } else {
                vErrors.push(err10);
              }
              errors++;
            }
            var valid3 = _errs14 === errors;
          } else {
            var valid3 = true;
          }
          if (valid3) {
            if (data.value !== undefined) {
              const _errs15 = errors;
              if (
                !validate83(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate83.errors : vErrors.concat(validate83.errors);
                errors = vErrors.length;
              }
              var valid3 = _errs15 === errors;
            } else {
              var valid3 = true;
            }
          }
        }
      }
    } else {
      const err11 = {
        instancePath,
        schemaPath: '#/anyOf/2/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err11];
      } else {
        vErrors.push(err11);
      }
      errors++;
    }
  }
  var _valid0 = _errs11 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs16 = errors;
  if (errors === _errs16) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing3;
      if (
        (data.command === undefined && (missing3 = 'command')) ||
        (data.value === undefined && (missing3 = 'value'))
      ) {
        const err12 = {
          instancePath,
          schemaPath: '#/anyOf/3/required',
          keyword: 'required',
          params: { missingProperty: missing3 },
          message: "must have required property '" + missing3 + "'",
        };
        if (vErrors === null) {
          vErrors = [err12];
        } else {
          vErrors.push(err12);
        }
        errors++;
      } else {
        const _errs18 = errors;
        for (const key3 in data) {
          if (!(key3 === 'command' || key3 === 'value')) {
            const err13 = {
              instancePath,
              schemaPath: '#/anyOf/3/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key3 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err13];
            } else {
              vErrors.push(err13);
            }
            errors++;
            break;
          }
        }
        if (_errs18 === errors) {
          if (data.command !== undefined) {
            const _errs19 = errors;
            if ('tools.credential.clear' !== data.command) {
              const err14 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/3/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.clear' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err14];
              } else {
                vErrors.push(err14);
              }
              errors++;
            }
            var valid4 = _errs19 === errors;
          } else {
            var valid4 = true;
          }
          if (valid4) {
            if (data.value !== undefined) {
              const _errs20 = errors;
              if (
                !validate83(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate83.errors : vErrors.concat(validate83.errors);
                errors = vErrors.length;
              }
              var valid4 = _errs20 === errors;
            } else {
              var valid4 = true;
            }
          }
        }
      }
    } else {
      const err15 = {
        instancePath,
        schemaPath: '#/anyOf/3/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err15];
      } else {
        vErrors.push(err15);
      }
      errors++;
    }
  }
  var _valid0 = _errs16 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs21 = errors;
  if (errors === _errs21) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing4;
      if (
        (data.command === undefined && (missing4 = 'command')) ||
        (data.value === undefined && (missing4 = 'value'))
      ) {
        const err16 = {
          instancePath,
          schemaPath: '#/anyOf/4/required',
          keyword: 'required',
          params: { missingProperty: missing4 },
          message: "must have required property '" + missing4 + "'",
        };
        if (vErrors === null) {
          vErrors = [err16];
        } else {
          vErrors.push(err16);
        }
        errors++;
      } else {
        const _errs23 = errors;
        for (const key4 in data) {
          if (!(key4 === 'command' || key4 === 'value')) {
            const err17 = {
              instancePath,
              schemaPath: '#/anyOf/4/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key4 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err17];
            } else {
              vErrors.push(err17);
            }
            errors++;
            break;
          }
        }
        if (_errs23 === errors) {
          if (data.command !== undefined) {
            const _errs24 = errors;
            if ('tools.credential.origins' !== data.command) {
              const err18 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/4/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.origins' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err18];
              } else {
                vErrors.push(err18);
              }
              errors++;
            }
            var valid5 = _errs24 === errors;
          } else {
            var valid5 = true;
          }
          if (valid5) {
            if (data.value !== undefined) {
              const _errs25 = errors;
              if (
                !validate90(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate90.errors : vErrors.concat(validate90.errors);
                errors = vErrors.length;
              }
              var valid5 = _errs25 === errors;
            } else {
              var valid5 = true;
            }
          }
        }
      }
    } else {
      const err19 = {
        instancePath,
        schemaPath: '#/anyOf/4/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err19];
      } else {
        vErrors.push(err19);
      }
      errors++;
    }
  }
  var _valid0 = _errs21 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs26 = errors;
  if (errors === _errs26) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing5;
      if (
        (data.command === undefined && (missing5 = 'command')) ||
        (data.value === undefined && (missing5 = 'value'))
      ) {
        const err20 = {
          instancePath,
          schemaPath: '#/anyOf/5/required',
          keyword: 'required',
          params: { missingProperty: missing5 },
          message: "must have required property '" + missing5 + "'",
        };
        if (vErrors === null) {
          vErrors = [err20];
        } else {
          vErrors.push(err20);
        }
        errors++;
      } else {
        const _errs28 = errors;
        for (const key5 in data) {
          if (!(key5 === 'command' || key5 === 'value')) {
            const err21 = {
              instancePath,
              schemaPath: '#/anyOf/5/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key5 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err21];
            } else {
              vErrors.push(err21);
            }
            errors++;
            break;
          }
        }
        if (_errs28 === errors) {
          if (data.command !== undefined) {
            const _errs29 = errors;
            if ('tools.credential.recover' !== data.command) {
              const err22 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/5/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.recover' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err22];
              } else {
                vErrors.push(err22);
              }
              errors++;
            }
            var valid6 = _errs29 === errors;
          } else {
            var valid6 = true;
          }
          if (valid6) {
            if (data.value !== undefined) {
              const _errs30 = errors;
              if (
                !validate83(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate83.errors : vErrors.concat(validate83.errors);
                errors = vErrors.length;
              }
              var valid6 = _errs30 === errors;
            } else {
              var valid6 = true;
            }
          }
        }
      }
    } else {
      const err23 = {
        instancePath,
        schemaPath: '#/anyOf/5/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err23];
      } else {
        vErrors.push(err23);
      }
      errors++;
    }
  }
  var _valid0 = _errs26 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs31 = errors;
  if (errors === _errs31) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing6;
      if (
        (data.command === undefined && (missing6 = 'command')) ||
        (data.value === undefined && (missing6 = 'value'))
      ) {
        const err24 = {
          instancePath,
          schemaPath: '#/anyOf/6/required',
          keyword: 'required',
          params: { missingProperty: missing6 },
          message: "must have required property '" + missing6 + "'",
        };
        if (vErrors === null) {
          vErrors = [err24];
        } else {
          vErrors.push(err24);
        }
        errors++;
      } else {
        const _errs33 = errors;
        for (const key6 in data) {
          if (!(key6 === 'command' || key6 === 'value')) {
            const err25 = {
              instancePath,
              schemaPath: '#/anyOf/6/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key6 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err25];
            } else {
              vErrors.push(err25);
            }
            errors++;
            break;
          }
        }
        if (_errs33 === errors) {
          if (data.command !== undefined) {
            const _errs34 = errors;
            if ('tools.credential.list' !== data.command) {
              const err26 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/6/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.list' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err26];
              } else {
                vErrors.push(err26);
              }
              errors++;
            }
            var valid7 = _errs34 === errors;
          } else {
            var valid7 = true;
          }
          if (valid7) {
            if (data.value !== undefined) {
              const _errs35 = errors;
              if (
                !validate56(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate56.errors : vErrors.concat(validate56.errors);
                errors = vErrors.length;
              }
              var valid7 = _errs35 === errors;
            } else {
              var valid7 = true;
            }
          }
        }
      }
    } else {
      const err27 = {
        instancePath,
        schemaPath: '#/anyOf/6/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err27];
      } else {
        vErrors.push(err27);
      }
      errors++;
    }
  }
  var _valid0 = _errs31 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  if (!valid0) {
    const err28 = {
      instancePath,
      schemaPath: '#/anyOf',
      keyword: 'anyOf',
      params: {},
      message: 'must match a schema in anyOf',
    };
    if (vErrors === null) {
      vErrors = [err28];
    } else {
      vErrors.push(err28);
    }
    errors++;
    validate52.errors = vErrors;
    return false;
  } else {
    errors = _errs0;
    if (vErrors !== null) {
      if (_errs0) {
        vErrors.length = _errs0;
      } else {
        vErrors = null;
      }
    }
  }
  validate52.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate52.evaluated = { dynamicProps: true, dynamicItems: false };
export const validateToolCredentialResponse = validate97;
const schema36 = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  anyOf: [
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.outcome' },
        value: { $ref: '#/$defs/ToolCredentialOutcomeV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.create' },
        value: { $ref: '#/$defs/ToolCredentialMutationResultV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.rotate' },
        value: { $ref: '#/$defs/ToolCredentialMutationResultV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.clear' },
        value: { $ref: '#/$defs/ToolCredentialMutationResultV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.origins' },
        value: { $ref: '#/$defs/ToolCredentialMutationResultV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.recover' },
        value: { $ref: '#/$defs/ToolCredentialMutationResultV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'tools.credential.list' },
        value: { $ref: '#/$defs/ToolCredentialListResultV1' },
      },
    },
  ],
  $defs: {
    ToolCredentialOutcomeRequestV1: {
      additionalProperties: false,
      properties: {
        command_invocation_id: { $ref: '#/$defs/CommandInvocationId' },
        target: { $ref: '#/$defs/ToolCredentialManagementTargetV1' },
      },
      required: ['target', 'command_invocation_id'],
      type: 'object',
    },
    CommandInvocationId: { type: 'string' },
    ToolCredentialManagementTargetV1: {
      additionalProperties: false,
      properties: {
        credential_slot_key: { $ref: '#/$defs/CredentialSlotKey' },
        expected_instance_revision: { $ref: '#/$defs/Revision' },
        placement: { $ref: '#/$defs/ToolCredentialPlacementV1' },
        session_id: { anyOf: [{ $ref: '#/$defs/SessionId' }, { type: 'null' }] },
        tool_instance_id: { $ref: '#/$defs/ToolInstanceId' },
        workspace_id: { anyOf: [{ $ref: '#/$defs/WorkspaceId' }, { type: 'null' }] },
      },
      required: [
        'tool_instance_id',
        'expected_instance_revision',
        'credential_slot_key',
        'placement',
      ],
      type: 'object',
    },
    CredentialSlotKey: {
      maxLength: 64,
      minLength: 1,
      pattern: '^[a-z0-9]+(?:[-_][a-z0-9]+)*$',
      type: 'string',
    },
    Revision: { type: 'string' },
    ToolCredentialPlacementV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            environment_variable: { $ref: '#/$defs/EnvironmentVariableNameV1' },
            placement: { const: 'command_environment', type: 'string' },
          },
          required: ['placement', 'environment_variable'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: { placement: { const: 'http_bearer_authorization', type: 'string' } },
          required: ['placement'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            header_name: { $ref: '#/$defs/HttpHeaderNameV1' },
            placement: { const: 'http_fixed_header', type: 'string' },
          },
          required: ['placement', 'header_name'],
          type: 'object',
        },
      ],
    },
    EnvironmentVariableNameV1: {
      maxLength: 64,
      minLength: 1,
      not: {
        enum: [
          'PATH',
          'PATHEXT',
          'PSMODULEPATH',
          'COMSPEC',
          'SYSTEMROOT',
          'WINDIR',
          'TEMP',
          'TMP',
          'LOCALAPPDATA',
          'AURA_TOOL_CALL_ID',
          'AURA_TOOL_EXECUTION_ATTEMPT_ID',
          'AURA_TOOL_OUTPUT_ROOT',
        ],
      },
      pattern: '^[A-Z_][A-Z0-9_]{0,63}$',
      type: 'string',
    },
    HttpHeaderNameV1: {
      maxLength: 128,
      minLength: 1,
      not: {
        pattern:
          '^(?:[Hh][Oo][Ss][Tt]|[Cc][Oo][Nn][Tt][Ee][Nn][Tt]-[Ll][Ee][Nn][Gg][Tt][Hh]|[Tt][Rr][Aa][Nn][Ss][Ff][Ee][Rr]-[Ee][Nn][Cc][Oo][Dd][Ii][Nn][Gg]|[Cc][Oo][Nn][Nn][Ee][Cc][Tt][Ii][Oo][Nn]|[Aa][Uu][Tt][Hh][Oo][Rr][Ii][Zz][Aa][Tt][Ii][Oo][Nn]|[Cc][Oo][Oo][Kk][Ii][Ee]|[Ss][Ee][Tt]-[Cc][Oo][Oo][Kk][Ii][Ee]|[Uu][Pp][Gg][Rr][Aa][Dd][Ee]|[Tt][Rr][Aa][Ii][Ll][Ee][Rr]|[Ee][Xx][Pp][Ee][Cc][Tt]|[Tt][Ee]|[Pp][Rr][Oo][Xx][Yy]-.*)$',
      },
      pattern: "^[!#$%&'*+.^_`|~0-9A-Za-z-]+$",
      type: 'string',
    },
    SessionId: { type: 'string' },
    ToolInstanceId: { type: 'string' },
    WorkspaceId: { type: 'string' },
    ToolCredentialOutcomeV1: {
      additionalProperties: false,
      properties: {
        command_invocation_id: { $ref: '#/$defs/CommandInvocationId' },
        result: { anyOf: [{ $ref: '#/$defs/ToolCredentialMutationResultV1' }, { type: 'null' }] },
        state: { $ref: '#/$defs/ToolCredentialOutcomeStateV1' },
      },
      required: ['command_invocation_id', 'state'],
      type: 'object',
    },
    ToolCredentialMutationResultV1: {
      additionalProperties: false,
      properties: {
        cleanup_pending: {
          description:
            'The authoritative change committed; deletion of an old WinCred value may still need retry.',
          type: 'boolean',
        },
        credential: {
          anyOf: [{ $ref: '#/$defs/ToolCredentialAuthorityRecordV1' }, { type: 'null' }],
        },
        disposition: { $ref: '#/$defs/ToolCredentialMutationDispositionV1' },
      },
      required: ['disposition', 'cleanup_pending'],
      type: 'object',
    },
    ToolCredentialAuthorityRecordV1: {
      additionalProperties: false,
      description:
        'Authority metadata for a Tool Credential. Secret bytes and WinCred\nlocators are deliberately held by the P02 storage binding, never by this\nrenderer-safe domain record.',
      properties: {
        allowed_origins: {
          description:
            'User-configured HTTP origins. Empty historical records authorize no model-selected URL.',
          items: { type: 'string' },
          type: 'array',
        },
        authenticated_owner: { $ref: '#/$defs/PrincipalId' },
        created_at: { $ref: '#/$defs/UtcTimestamp' },
        credential_id: { $ref: '#/$defs/CredentialId' },
        credential_kind: { $ref: '#/$defs/ToolCredentialKindV1' },
        credential_revision: { $ref: '#/$defs/CredentialRevision' },
        display_name: { type: 'string' },
        placement_policy: { $ref: '#/$defs/ToolCredentialPlacementPolicyV1' },
        row_revision: { $ref: '#/$defs/Revision' },
        scope: { $ref: '#/$defs/ToolCredentialScopeV1' },
        state: { $ref: '#/$defs/ToolCredentialStateV1' },
        updated_at: { $ref: '#/$defs/UtcTimestamp' },
        usage_policy: { $ref: '#/$defs/ToolCredentialUsagePolicyV1' },
      },
      required: [
        'credential_id',
        'credential_revision',
        'authenticated_owner',
        'scope',
        'credential_kind',
        'state',
        'placement_policy',
        'usage_policy',
        'display_name',
        'row_revision',
        'created_at',
        'updated_at',
      ],
      type: 'object',
    },
    PrincipalId: { type: 'string' },
    UtcTimestamp: { type: 'string' },
    CredentialId: { type: 'string' },
    ToolCredentialKindV1: { enum: ['api_key'], type: 'string' },
    CredentialRevision: {
      $ref: '#/$defs/Revision',
      description:
        'Secret-generation revision for one stable CredentialId.\n\nThis is intentionally distinct from a row revision. Clear and metadata-only\nmutations may change row concurrency without creating a new secret\ngeneration.',
    },
    ToolCredentialPlacementPolicyV1: {
      additionalProperties: false,
      properties: {
        allowed_placements: { items: { $ref: '#/$defs/ToolCredentialPlacementV1' }, type: 'array' },
      },
      required: ['allowed_placements'],
      type: 'object',
    },
    ToolCredentialScopeV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: { scope: { const: 'user', type: 'string' } },
          required: ['scope'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            scope: { const: 'workspace', type: 'string' },
            workspace_id: { $ref: '#/$defs/WorkspaceId' },
          },
          required: ['scope', 'workspace_id'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            scope: { const: 'session', type: 'string' },
            session_id: { $ref: '#/$defs/SessionId' },
            workspace_id: { $ref: '#/$defs/WorkspaceId' },
          },
          required: ['scope', 'workspace_id', 'session_id'],
          type: 'object',
        },
      ],
    },
    ToolCredentialStateV1: {
      enum: ['configured', 'suspended', 'revoked', 'recovery_required'],
      type: 'string',
    },
    ToolCredentialUsagePolicyV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            policy: { const: 'explicit', type: 'string' },
            tool_ids: { items: { $ref: '#/$defs/ToolId' }, type: 'array' },
            tool_package_ids: { items: { $ref: '#/$defs/ToolPackageId' }, type: 'array' },
          },
          required: ['policy', 'tool_package_ids', 'tool_ids'],
          type: 'object',
        },
      ],
    },
    ToolId: { type: 'string' },
    ToolPackageId: { type: 'string' },
    ToolCredentialMutationDispositionV1: { enum: ['committed', 'aborted'], type: 'string' },
    ToolCredentialOutcomeStateV1: {
      enum: ['pending', 'committed', 'aborted', 'not_obtained'],
      type: 'string',
    },
    ToolCredentialCreateRequestV1: {
      additionalProperties: false,
      properties: {
        allowed_origins: { items: { type: 'string' }, type: 'array' },
        display_name: { type: 'string' },
        scope: { $ref: '#/$defs/ToolCredentialScopeV1' },
        target: { $ref: '#/$defs/ToolCredentialManagementTargetV1' },
      },
      required: ['target', 'display_name', 'scope'],
      type: 'object',
    },
    ToolCredentialChangeRequestV1: {
      additionalProperties: false,
      properties: {
        credential_id: { $ref: '#/$defs/CredentialId' },
        expected_row_revision: { $ref: '#/$defs/Revision' },
        target: { $ref: '#/$defs/ToolCredentialManagementTargetV1' },
      },
      required: ['target', 'credential_id', 'expected_row_revision'],
      type: 'object',
    },
    ToolCredentialOriginsRequestV1: {
      additionalProperties: false,
      properties: {
        allowed_origins: { items: { type: 'string' }, type: 'array' },
        credential_id: { $ref: '#/$defs/CredentialId' },
        expected_row_revision: { $ref: '#/$defs/Revision' },
        target: { $ref: '#/$defs/ToolCredentialManagementTargetV1' },
      },
      required: ['target', 'credential_id', 'expected_row_revision', 'allowed_origins'],
      type: 'object',
    },
    ToolCredentialListResultV1: {
      additionalProperties: false,
      properties: {
        credentials: { items: { $ref: '#/$defs/ToolCredentialAuthorityRecordV1' }, type: 'array' },
        pending_credential_ids: { items: { $ref: '#/$defs/CredentialId' }, type: 'array' },
      },
      required: ['credentials', 'pending_credential_ids'],
      type: 'object',
    },
  },
};
const schema37 = {
  additionalProperties: false,
  properties: {
    command_invocation_id: { $ref: '#/$defs/CommandInvocationId' },
    result: { anyOf: [{ $ref: '#/$defs/ToolCredentialMutationResultV1' }, { type: 'null' }] },
    state: { $ref: '#/$defs/ToolCredentialOutcomeStateV1' },
  },
  required: ['command_invocation_id', 'state'],
  type: 'object',
};
function validate99(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate99.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate99.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate99.errors = vErrors;
  return errors === 0;
}
validate99.evaluated = { dynamicProps: false, dynamicItems: false };
const schema39 = {
  additionalProperties: false,
  properties: {
    cleanup_pending: {
      description:
        'The authoritative change committed; deletion of an old WinCred value may still need retry.',
      type: 'boolean',
    },
    credential: { anyOf: [{ $ref: '#/$defs/ToolCredentialAuthorityRecordV1' }, { type: 'null' }] },
    disposition: { $ref: '#/$defs/ToolCredentialMutationDispositionV1' },
  },
  required: ['disposition', 'cleanup_pending'],
  type: 'object',
};
const schema40 = {
  additionalProperties: false,
  description:
    'Authority metadata for a Tool Credential. Secret bytes and WinCred\nlocators are deliberately held by the P02 storage binding, never by this\nrenderer-safe domain record.',
  properties: {
    allowed_origins: {
      description:
        'User-configured HTTP origins. Empty historical records authorize no model-selected URL.',
      items: { type: 'string' },
      type: 'array',
    },
    authenticated_owner: { $ref: '#/$defs/PrincipalId' },
    created_at: { $ref: '#/$defs/UtcTimestamp' },
    credential_id: { $ref: '#/$defs/CredentialId' },
    credential_kind: { $ref: '#/$defs/ToolCredentialKindV1' },
    credential_revision: { $ref: '#/$defs/CredentialRevision' },
    display_name: { type: 'string' },
    placement_policy: { $ref: '#/$defs/ToolCredentialPlacementPolicyV1' },
    row_revision: { $ref: '#/$defs/Revision' },
    scope: { $ref: '#/$defs/ToolCredentialScopeV1' },
    state: { $ref: '#/$defs/ToolCredentialStateV1' },
    updated_at: { $ref: '#/$defs/UtcTimestamp' },
    usage_policy: { $ref: '#/$defs/ToolCredentialUsagePolicyV1' },
  },
  required: [
    'credential_id',
    'credential_revision',
    'authenticated_owner',
    'scope',
    'credential_kind',
    'state',
    'placement_policy',
    'usage_policy',
    'display_name',
    'row_revision',
    'created_at',
    'updated_at',
  ],
  type: 'object',
};
const func7 = Object.prototype.hasOwnProperty;
const schema41 = { type: 'string' };
function validate103(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate103.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate103.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate103.errors = vErrors;
  return errors === 0;
}
validate103.evaluated = { dynamicProps: false, dynamicItems: false };
const schema42 = { type: 'string' };
function validate105(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate105.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate105.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate105.errors = vErrors;
  return errors === 0;
}
validate105.evaluated = { dynamicProps: false, dynamicItems: false };
function validate107(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate107.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate107.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate107.errors = vErrors;
  return errors === 0;
}
validate107.evaluated = { dynamicProps: false, dynamicItems: false };
const schema44 = { enum: ['api_key'], type: 'string' };
function validate109(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate109.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate109.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  if (!(data === 'api_key')) {
    validate109.errors = [
      {
        instancePath,
        schemaPath: '#/enum',
        keyword: 'enum',
        params: { allowedValues: schema44.enum },
        message: 'must be equal to one of the allowed values',
      },
    ];
    return false;
  }
  validate109.errors = vErrors;
  return errors === 0;
}
validate109.evaluated = { dynamicProps: false, dynamicItems: false };
function validate111(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate111.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate111.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate111.errors = vErrors;
  return errors === 0;
}
validate111.evaluated = { dynamicProps: false, dynamicItems: false };
const schema46 = {
  additionalProperties: false,
  properties: {
    allowed_placements: { items: { $ref: '#/$defs/ToolCredentialPlacementV1' }, type: 'array' },
  },
  required: ['allowed_placements'],
  type: 'object',
};
function validate115(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate115.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs1 = errors;
  const _errs2 = errors;
  if (!(
    data === 'PATH' ||
    data === 'PATHEXT' ||
    data === 'PSMODULEPATH' ||
    data === 'COMSPEC' ||
    data === 'SYSTEMROOT' ||
    data === 'WINDIR' ||
    data === 'TEMP' ||
    data === 'TMP' ||
    data === 'LOCALAPPDATA' ||
    data === 'AURA_TOOL_CALL_ID' ||
    data === 'AURA_TOOL_EXECUTION_ATTEMPT_ID' ||
    data === 'AURA_TOOL_OUTPUT_ROOT'
  )) {
    const err0 = {};
    if (vErrors === null) {
      vErrors = [err0];
    } else {
      vErrors.push(err0);
    }
    errors++;
  }
  var valid0 = _errs2 === errors;
  if (valid0) {
    validate115.errors = [
      {
        instancePath,
        schemaPath: '#/not',
        keyword: 'not',
        params: {},
        message: 'must NOT be valid',
      },
    ];
    return false;
  } else {
    errors = _errs1;
    if (vErrors !== null) {
      if (_errs1) {
        vErrors.length = _errs1;
      } else {
        vErrors = null;
      }
    }
  }
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 64) {
        validate115.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 64 },
            message: 'must NOT have more than 64 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 1) {
          validate115.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 1 },
              message: 'must NOT have fewer than 1 characters',
            },
          ];
          return false;
        } else {
          if (!pattern4.test(data)) {
            validate115.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: '^[A-Z_][A-Z0-9_]{0,63}$' },
                message: 'must match pattern "' + '^[A-Z_][A-Z0-9_]{0,63}$' + '"',
              },
            ];
            return false;
          }
        }
      }
    } else {
      validate115.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'string' },
          message: 'must be string',
        },
      ];
      return false;
    }
  }
  validate115.errors = vErrors;
  return errors === 0;
}
validate115.evaluated = { dynamicProps: false, dynamicItems: false };
function validate117(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate117.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs1 = errors;
  const _errs2 = errors;
  if (typeof data === 'string') {
    if (!pattern5.test(data)) {
      const err0 = {};
      if (vErrors === null) {
        vErrors = [err0];
      } else {
        vErrors.push(err0);
      }
      errors++;
    }
  }
  var valid0 = _errs2 === errors;
  if (valid0) {
    validate117.errors = [
      {
        instancePath,
        schemaPath: '#/not',
        keyword: 'not',
        params: {},
        message: 'must NOT be valid',
      },
    ];
    return false;
  } else {
    errors = _errs1;
    if (vErrors !== null) {
      if (_errs1) {
        vErrors.length = _errs1;
      } else {
        vErrors = null;
      }
    }
  }
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 128) {
        validate117.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 128 },
            message: 'must NOT have more than 128 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 1) {
          validate117.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 1 },
              message: 'must NOT have fewer than 1 characters',
            },
          ];
          return false;
        } else {
          if (!pattern6.test(data)) {
            validate117.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: "^[!#$%&'*+.^_`|~0-9A-Za-z-]+$" },
                message: 'must match pattern "' + "^[!#$%&'*+.^_`|~0-9A-Za-z-]+$" + '"',
              },
            ];
            return false;
          }
        }
      }
    } else {
      validate117.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'string' },
          message: 'must be string',
        },
      ];
      return false;
    }
  }
  validate117.errors = vErrors;
  return errors === 0;
}
validate117.evaluated = { dynamicProps: false, dynamicItems: false };
function validate114(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate114.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs0 = errors;
  let valid0 = false;
  let passing0 = null;
  const _errs1 = errors;
  if (errors === _errs1) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.placement === undefined && (missing0 = 'placement')) ||
        (data.environment_variable === undefined && (missing0 = 'environment_variable'))
      ) {
        const err0 = {
          instancePath,
          schemaPath: '#/oneOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs3 = errors;
        for (const key0 in data) {
          if (!(key0 === 'environment_variable' || key0 === 'placement')) {
            const err1 = {
              instancePath,
              schemaPath: '#/oneOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs3 === errors) {
          if (data.environment_variable !== undefined) {
            const _errs4 = errors;
            if (
              !validate115(data.environment_variable, {
                instancePath: instancePath + '/environment_variable',
                parentData: data,
                parentDataProperty: 'environment_variable',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate115.errors : vErrors.concat(validate115.errors);
              errors = vErrors.length;
            }
            var valid1 = _errs4 === errors;
          } else {
            var valid1 = true;
          }
          if (valid1) {
            if (data.placement !== undefined) {
              let data1 = data.placement;
              const _errs5 = errors;
              if (typeof data1 !== 'string') {
                const err2 = {
                  instancePath: instancePath + '/placement',
                  schemaPath: '#/oneOf/0/properties/placement/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                };
                if (vErrors === null) {
                  vErrors = [err2];
                } else {
                  vErrors.push(err2);
                }
                errors++;
              }
              if ('command_environment' !== data1) {
                const err3 = {
                  instancePath: instancePath + '/placement',
                  schemaPath: '#/oneOf/0/properties/placement/const',
                  keyword: 'const',
                  params: { allowedValue: 'command_environment' },
                  message: 'must be equal to constant',
                };
                if (vErrors === null) {
                  vErrors = [err3];
                } else {
                  vErrors.push(err3);
                }
                errors++;
              }
              var valid1 = _errs5 === errors;
            } else {
              var valid1 = true;
            }
          }
        }
      }
    } else {
      const err4 = {
        instancePath,
        schemaPath: '#/oneOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err4];
      } else {
        vErrors.push(err4);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  if (_valid0) {
    valid0 = true;
    passing0 = 0;
    var props0 = true;
  }
  const _errs7 = errors;
  if (errors === _errs7) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (data.placement === undefined && (missing1 = 'placement')) {
        const err5 = {
          instancePath,
          schemaPath: '#/oneOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err5];
        } else {
          vErrors.push(err5);
        }
        errors++;
      } else {
        const _errs9 = errors;
        for (const key1 in data) {
          if (!(key1 === 'placement')) {
            const err6 = {
              instancePath,
              schemaPath: '#/oneOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err6];
            } else {
              vErrors.push(err6);
            }
            errors++;
            break;
          }
        }
        if (_errs9 === errors) {
          if (data.placement !== undefined) {
            let data2 = data.placement;
            if (typeof data2 !== 'string') {
              const err7 = {
                instancePath: instancePath + '/placement',
                schemaPath: '#/oneOf/1/properties/placement/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err7];
              } else {
                vErrors.push(err7);
              }
              errors++;
            }
            if ('http_bearer_authorization' !== data2) {
              const err8 = {
                instancePath: instancePath + '/placement',
                schemaPath: '#/oneOf/1/properties/placement/const',
                keyword: 'const',
                params: { allowedValue: 'http_bearer_authorization' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err8];
              } else {
                vErrors.push(err8);
              }
              errors++;
            }
          }
        }
      }
    } else {
      const err9 = {
        instancePath,
        schemaPath: '#/oneOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err9];
      } else {
        vErrors.push(err9);
      }
      errors++;
    }
  }
  var _valid0 = _errs7 === errors;
  if (_valid0 && valid0) {
    valid0 = false;
    passing0 = [passing0, 1];
  } else {
    if (_valid0) {
      valid0 = true;
      passing0 = 1;
      if (props0 !== true) {
        props0 = true;
      }
    }
    const _errs12 = errors;
    if (errors === _errs12) {
      if (data && typeof data == 'object' && !Array.isArray(data)) {
        let missing2;
        if (
          (data.placement === undefined && (missing2 = 'placement')) ||
          (data.header_name === undefined && (missing2 = 'header_name'))
        ) {
          const err10 = {
            instancePath,
            schemaPath: '#/oneOf/2/required',
            keyword: 'required',
            params: { missingProperty: missing2 },
            message: "must have required property '" + missing2 + "'",
          };
          if (vErrors === null) {
            vErrors = [err10];
          } else {
            vErrors.push(err10);
          }
          errors++;
        } else {
          const _errs14 = errors;
          for (const key2 in data) {
            if (!(key2 === 'header_name' || key2 === 'placement')) {
              const err11 = {
                instancePath,
                schemaPath: '#/oneOf/2/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key2 },
                message: 'must NOT have additional properties',
              };
              if (vErrors === null) {
                vErrors = [err11];
              } else {
                vErrors.push(err11);
              }
              errors++;
              break;
            }
          }
          if (_errs14 === errors) {
            if (data.header_name !== undefined) {
              const _errs15 = errors;
              if (
                !validate117(data.header_name, {
                  instancePath: instancePath + '/header_name',
                  parentData: data,
                  parentDataProperty: 'header_name',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate117.errors : vErrors.concat(validate117.errors);
                errors = vErrors.length;
              }
              var valid3 = _errs15 === errors;
            } else {
              var valid3 = true;
            }
            if (valid3) {
              if (data.placement !== undefined) {
                let data4 = data.placement;
                const _errs16 = errors;
                if (typeof data4 !== 'string') {
                  const err12 = {
                    instancePath: instancePath + '/placement',
                    schemaPath: '#/oneOf/2/properties/placement/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  };
                  if (vErrors === null) {
                    vErrors = [err12];
                  } else {
                    vErrors.push(err12);
                  }
                  errors++;
                }
                if ('http_fixed_header' !== data4) {
                  const err13 = {
                    instancePath: instancePath + '/placement',
                    schemaPath: '#/oneOf/2/properties/placement/const',
                    keyword: 'const',
                    params: { allowedValue: 'http_fixed_header' },
                    message: 'must be equal to constant',
                  };
                  if (vErrors === null) {
                    vErrors = [err13];
                  } else {
                    vErrors.push(err13);
                  }
                  errors++;
                }
                var valid3 = _errs16 === errors;
              } else {
                var valid3 = true;
              }
            }
          }
        }
      } else {
        const err14 = {
          instancePath,
          schemaPath: '#/oneOf/2/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        };
        if (vErrors === null) {
          vErrors = [err14];
        } else {
          vErrors.push(err14);
        }
        errors++;
      }
    }
    var _valid0 = _errs12 === errors;
    if (_valid0 && valid0) {
      valid0 = false;
      passing0 = [passing0, 2];
    } else {
      if (_valid0) {
        valid0 = true;
        passing0 = 2;
        if (props0 !== true) {
          props0 = true;
        }
      }
    }
  }
  if (!valid0) {
    const err15 = {
      instancePath,
      schemaPath: '#/oneOf',
      keyword: 'oneOf',
      params: { passingSchemas: passing0 },
      message: 'must match exactly one schema in oneOf',
    };
    if (vErrors === null) {
      vErrors = [err15];
    } else {
      vErrors.push(err15);
    }
    errors++;
    validate114.errors = vErrors;
    return false;
  } else {
    errors = _errs0;
    if (vErrors !== null) {
      if (_errs0) {
        vErrors.length = _errs0;
      } else {
        vErrors = null;
      }
    }
  }
  validate114.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate114.evaluated = { dynamicProps: true, dynamicItems: false };
function validate113(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate113.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (data.allowed_placements === undefined && (missing0 = 'allowed_placements')) {
        validate113.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(key0 === 'allowed_placements')) {
            validate113.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.allowed_placements !== undefined) {
            let data0 = data.allowed_placements;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  const _errs4 = errors;
                  if (
                    !validate114(data0[i0], {
                      instancePath: instancePath + '/allowed_placements/' + i0,
                      parentData: data0,
                      parentDataProperty: i0,
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate114.errors : vErrors.concat(validate114.errors);
                    errors = vErrors.length;
                  }
                  var valid1 = _errs4 === errors;
                  if (!valid1) {
                    break;
                  }
                }
              } else {
                validate113.errors = [
                  {
                    instancePath: instancePath + '/allowed_placements',
                    schemaPath: '#/properties/allowed_placements/type',
                    keyword: 'type',
                    params: { type: 'array' },
                    message: 'must be array',
                  },
                ];
                return false;
              }
            }
          }
        }
      }
    } else {
      validate113.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate113.errors = vErrors;
  return errors === 0;
}
validate113.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate121(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate121.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate121.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate121.errors = vErrors;
  return errors === 0;
}
validate121.evaluated = { dynamicProps: false, dynamicItems: false };
function validate124(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate124.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate124.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate124.errors = vErrors;
  return errors === 0;
}
validate124.evaluated = { dynamicProps: false, dynamicItems: false };
function validate126(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate126.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate126.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate126.errors = vErrors;
  return errors === 0;
}
validate126.evaluated = { dynamicProps: false, dynamicItems: false };
function validate123(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate123.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs0 = errors;
  let valid0 = false;
  let passing0 = null;
  const _errs1 = errors;
  if (errors === _errs1) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (data.scope === undefined && (missing0 = 'scope')) {
        const err0 = {
          instancePath,
          schemaPath: '#/oneOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs3 = errors;
        for (const key0 in data) {
          if (!(key0 === 'scope')) {
            const err1 = {
              instancePath,
              schemaPath: '#/oneOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs3 === errors) {
          if (data.scope !== undefined) {
            let data0 = data.scope;
            if (typeof data0 !== 'string') {
              const err2 = {
                instancePath: instancePath + '/scope',
                schemaPath: '#/oneOf/0/properties/scope/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err2];
              } else {
                vErrors.push(err2);
              }
              errors++;
            }
            if ('user' !== data0) {
              const err3 = {
                instancePath: instancePath + '/scope',
                schemaPath: '#/oneOf/0/properties/scope/const',
                keyword: 'const',
                params: { allowedValue: 'user' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err3];
              } else {
                vErrors.push(err3);
              }
              errors++;
            }
          }
        }
      }
    } else {
      const err4 = {
        instancePath,
        schemaPath: '#/oneOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err4];
      } else {
        vErrors.push(err4);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  if (_valid0) {
    valid0 = true;
    passing0 = 0;
    var props0 = true;
  }
  const _errs6 = errors;
  if (errors === _errs6) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (
        (data.scope === undefined && (missing1 = 'scope')) ||
        (data.workspace_id === undefined && (missing1 = 'workspace_id'))
      ) {
        const err5 = {
          instancePath,
          schemaPath: '#/oneOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err5];
        } else {
          vErrors.push(err5);
        }
        errors++;
      } else {
        const _errs8 = errors;
        for (const key1 in data) {
          if (!(key1 === 'scope' || key1 === 'workspace_id')) {
            const err6 = {
              instancePath,
              schemaPath: '#/oneOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err6];
            } else {
              vErrors.push(err6);
            }
            errors++;
            break;
          }
        }
        if (_errs8 === errors) {
          if (data.scope !== undefined) {
            let data1 = data.scope;
            const _errs9 = errors;
            if (typeof data1 !== 'string') {
              const err7 = {
                instancePath: instancePath + '/scope',
                schemaPath: '#/oneOf/1/properties/scope/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err7];
              } else {
                vErrors.push(err7);
              }
              errors++;
            }
            if ('workspace' !== data1) {
              const err8 = {
                instancePath: instancePath + '/scope',
                schemaPath: '#/oneOf/1/properties/scope/const',
                keyword: 'const',
                params: { allowedValue: 'workspace' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err8];
              } else {
                vErrors.push(err8);
              }
              errors++;
            }
            var valid2 = _errs9 === errors;
          } else {
            var valid2 = true;
          }
          if (valid2) {
            if (data.workspace_id !== undefined) {
              const _errs11 = errors;
              if (
                !validate124(data.workspace_id, {
                  instancePath: instancePath + '/workspace_id',
                  parentData: data,
                  parentDataProperty: 'workspace_id',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate124.errors : vErrors.concat(validate124.errors);
                errors = vErrors.length;
              }
              var valid2 = _errs11 === errors;
            } else {
              var valid2 = true;
            }
          }
        }
      }
    } else {
      const err9 = {
        instancePath,
        schemaPath: '#/oneOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err9];
      } else {
        vErrors.push(err9);
      }
      errors++;
    }
  }
  var _valid0 = _errs6 === errors;
  if (_valid0 && valid0) {
    valid0 = false;
    passing0 = [passing0, 1];
  } else {
    if (_valid0) {
      valid0 = true;
      passing0 = 1;
      if (props0 !== true) {
        props0 = true;
      }
    }
    const _errs12 = errors;
    if (errors === _errs12) {
      if (data && typeof data == 'object' && !Array.isArray(data)) {
        let missing2;
        if (
          (data.scope === undefined && (missing2 = 'scope')) ||
          (data.workspace_id === undefined && (missing2 = 'workspace_id')) ||
          (data.session_id === undefined && (missing2 = 'session_id'))
        ) {
          const err10 = {
            instancePath,
            schemaPath: '#/oneOf/2/required',
            keyword: 'required',
            params: { missingProperty: missing2 },
            message: "must have required property '" + missing2 + "'",
          };
          if (vErrors === null) {
            vErrors = [err10];
          } else {
            vErrors.push(err10);
          }
          errors++;
        } else {
          const _errs14 = errors;
          for (const key2 in data) {
            if (!(key2 === 'scope' || key2 === 'session_id' || key2 === 'workspace_id')) {
              const err11 = {
                instancePath,
                schemaPath: '#/oneOf/2/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key2 },
                message: 'must NOT have additional properties',
              };
              if (vErrors === null) {
                vErrors = [err11];
              } else {
                vErrors.push(err11);
              }
              errors++;
              break;
            }
          }
          if (_errs14 === errors) {
            if (data.scope !== undefined) {
              let data3 = data.scope;
              const _errs15 = errors;
              if (typeof data3 !== 'string') {
                const err12 = {
                  instancePath: instancePath + '/scope',
                  schemaPath: '#/oneOf/2/properties/scope/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                };
                if (vErrors === null) {
                  vErrors = [err12];
                } else {
                  vErrors.push(err12);
                }
                errors++;
              }
              if ('session' !== data3) {
                const err13 = {
                  instancePath: instancePath + '/scope',
                  schemaPath: '#/oneOf/2/properties/scope/const',
                  keyword: 'const',
                  params: { allowedValue: 'session' },
                  message: 'must be equal to constant',
                };
                if (vErrors === null) {
                  vErrors = [err13];
                } else {
                  vErrors.push(err13);
                }
                errors++;
              }
              var valid3 = _errs15 === errors;
            } else {
              var valid3 = true;
            }
            if (valid3) {
              if (data.session_id !== undefined) {
                const _errs17 = errors;
                if (
                  !validate126(data.session_id, {
                    instancePath: instancePath + '/session_id',
                    parentData: data,
                    parentDataProperty: 'session_id',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate126.errors : vErrors.concat(validate126.errors);
                  errors = vErrors.length;
                }
                var valid3 = _errs17 === errors;
              } else {
                var valid3 = true;
              }
              if (valid3) {
                if (data.workspace_id !== undefined) {
                  const _errs18 = errors;
                  if (
                    !validate124(data.workspace_id, {
                      instancePath: instancePath + '/workspace_id',
                      parentData: data,
                      parentDataProperty: 'workspace_id',
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate124.errors : vErrors.concat(validate124.errors);
                    errors = vErrors.length;
                  }
                  var valid3 = _errs18 === errors;
                } else {
                  var valid3 = true;
                }
              }
            }
          }
        }
      } else {
        const err14 = {
          instancePath,
          schemaPath: '#/oneOf/2/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        };
        if (vErrors === null) {
          vErrors = [err14];
        } else {
          vErrors.push(err14);
        }
        errors++;
      }
    }
    var _valid0 = _errs12 === errors;
    if (_valid0 && valid0) {
      valid0 = false;
      passing0 = [passing0, 2];
    } else {
      if (_valid0) {
        valid0 = true;
        passing0 = 2;
        if (props0 !== true) {
          props0 = true;
        }
      }
    }
  }
  if (!valid0) {
    const err15 = {
      instancePath,
      schemaPath: '#/oneOf',
      keyword: 'oneOf',
      params: { passingSchemas: passing0 },
      message: 'must match exactly one schema in oneOf',
    };
    if (vErrors === null) {
      vErrors = [err15];
    } else {
      vErrors.push(err15);
    }
    errors++;
    validate123.errors = vErrors;
    return false;
  } else {
    errors = _errs0;
    if (vErrors !== null) {
      if (_errs0) {
        vErrors.length = _errs0;
      } else {
        vErrors = null;
      }
    }
  }
  validate123.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate123.evaluated = { dynamicProps: true, dynamicItems: false };
const schema54 = {
  enum: ['configured', 'suspended', 'revoked', 'recovery_required'],
  type: 'string',
};
function validate130(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate130.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate130.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  if (!(
    data === 'configured' ||
    data === 'suspended' ||
    data === 'revoked' ||
    data === 'recovery_required'
  )) {
    validate130.errors = [
      {
        instancePath,
        schemaPath: '#/enum',
        keyword: 'enum',
        params: { allowedValues: schema54.enum },
        message: 'must be equal to one of the allowed values',
      },
    ];
    return false;
  }
  validate130.errors = vErrors;
  return errors === 0;
}
validate130.evaluated = { dynamicProps: false, dynamicItems: false };
const schema55 = {
  oneOf: [
    {
      additionalProperties: false,
      properties: {
        policy: { const: 'explicit', type: 'string' },
        tool_ids: { items: { $ref: '#/$defs/ToolId' }, type: 'array' },
        tool_package_ids: { items: { $ref: '#/$defs/ToolPackageId' }, type: 'array' },
      },
      required: ['policy', 'tool_package_ids', 'tool_ids'],
      type: 'object',
    },
  ],
};
const schema56 = { type: 'string' };
function validate134(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate134.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate134.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate134.errors = vErrors;
  return errors === 0;
}
validate134.evaluated = { dynamicProps: false, dynamicItems: false };
const schema57 = { type: 'string' };
function validate136(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate136.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate136.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  validate136.errors = vErrors;
  return errors === 0;
}
validate136.evaluated = { dynamicProps: false, dynamicItems: false };
function validate133(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate133.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs0 = errors;
  let valid0 = false;
  let passing0 = null;
  const _errs1 = errors;
  if (errors === _errs1) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.policy === undefined && (missing0 = 'policy')) ||
        (data.tool_package_ids === undefined && (missing0 = 'tool_package_ids')) ||
        (data.tool_ids === undefined && (missing0 = 'tool_ids'))
      ) {
        const err0 = {
          instancePath,
          schemaPath: '#/oneOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs3 = errors;
        for (const key0 in data) {
          if (!(key0 === 'policy' || key0 === 'tool_ids' || key0 === 'tool_package_ids')) {
            const err1 = {
              instancePath,
              schemaPath: '#/oneOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs3 === errors) {
          if (data.policy !== undefined) {
            let data0 = data.policy;
            const _errs4 = errors;
            if (typeof data0 !== 'string') {
              const err2 = {
                instancePath: instancePath + '/policy',
                schemaPath: '#/oneOf/0/properties/policy/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err2];
              } else {
                vErrors.push(err2);
              }
              errors++;
            }
            if ('explicit' !== data0) {
              const err3 = {
                instancePath: instancePath + '/policy',
                schemaPath: '#/oneOf/0/properties/policy/const',
                keyword: 'const',
                params: { allowedValue: 'explicit' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err3];
              } else {
                vErrors.push(err3);
              }
              errors++;
            }
            var valid1 = _errs4 === errors;
          } else {
            var valid1 = true;
          }
          if (valid1) {
            if (data.tool_ids !== undefined) {
              let data1 = data.tool_ids;
              const _errs6 = errors;
              if (errors === _errs6) {
                if (Array.isArray(data1)) {
                  var valid2 = true;
                  const len0 = data1.length;
                  for (let i0 = 0; i0 < len0; i0++) {
                    const _errs8 = errors;
                    if (
                      !validate134(data1[i0], {
                        instancePath: instancePath + '/tool_ids/' + i0,
                        parentData: data1,
                        parentDataProperty: i0,
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate134.errors : vErrors.concat(validate134.errors);
                      errors = vErrors.length;
                    }
                    var valid2 = _errs8 === errors;
                    if (!valid2) {
                      break;
                    }
                  }
                } else {
                  const err4 = {
                    instancePath: instancePath + '/tool_ids',
                    schemaPath: '#/oneOf/0/properties/tool_ids/type',
                    keyword: 'type',
                    params: { type: 'array' },
                    message: 'must be array',
                  };
                  if (vErrors === null) {
                    vErrors = [err4];
                  } else {
                    vErrors.push(err4);
                  }
                  errors++;
                }
              }
              var valid1 = _errs6 === errors;
            } else {
              var valid1 = true;
            }
            if (valid1) {
              if (data.tool_package_ids !== undefined) {
                let data3 = data.tool_package_ids;
                const _errs9 = errors;
                if (errors === _errs9) {
                  if (Array.isArray(data3)) {
                    var valid3 = true;
                    const len1 = data3.length;
                    for (let i1 = 0; i1 < len1; i1++) {
                      const _errs11 = errors;
                      if (
                        !validate136(data3[i1], {
                          instancePath: instancePath + '/tool_package_ids/' + i1,
                          parentData: data3,
                          parentDataProperty: i1,
                          rootData,
                          dynamicAnchors,
                        })
                      ) {
                        vErrors =
                          vErrors === null
                            ? validate136.errors
                            : vErrors.concat(validate136.errors);
                        errors = vErrors.length;
                      }
                      var valid3 = _errs11 === errors;
                      if (!valid3) {
                        break;
                      }
                    }
                  } else {
                    const err5 = {
                      instancePath: instancePath + '/tool_package_ids',
                      schemaPath: '#/oneOf/0/properties/tool_package_ids/type',
                      keyword: 'type',
                      params: { type: 'array' },
                      message: 'must be array',
                    };
                    if (vErrors === null) {
                      vErrors = [err5];
                    } else {
                      vErrors.push(err5);
                    }
                    errors++;
                  }
                }
                var valid1 = _errs9 === errors;
              } else {
                var valid1 = true;
              }
            }
          }
        }
      }
    } else {
      const err6 = {
        instancePath,
        schemaPath: '#/oneOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err6];
      } else {
        vErrors.push(err6);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  if (_valid0) {
    valid0 = true;
    passing0 = 0;
    var props0 = true;
  }
  if (!valid0) {
    const err7 = {
      instancePath,
      schemaPath: '#/oneOf',
      keyword: 'oneOf',
      params: { passingSchemas: passing0 },
      message: 'must match exactly one schema in oneOf',
    };
    if (vErrors === null) {
      vErrors = [err7];
    } else {
      vErrors.push(err7);
    }
    errors++;
    validate133.errors = vErrors;
    return false;
  } else {
    errors = _errs0;
    if (vErrors !== null) {
      if (_errs0) {
        vErrors.length = _errs0;
      } else {
        vErrors = null;
      }
    }
  }
  validate133.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate133.evaluated = { dynamicProps: true, dynamicItems: false };
function validate102(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate102.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.credential_id === undefined && (missing0 = 'credential_id')) ||
        (data.credential_revision === undefined && (missing0 = 'credential_revision')) ||
        (data.authenticated_owner === undefined && (missing0 = 'authenticated_owner')) ||
        (data.scope === undefined && (missing0 = 'scope')) ||
        (data.credential_kind === undefined && (missing0 = 'credential_kind')) ||
        (data.state === undefined && (missing0 = 'state')) ||
        (data.placement_policy === undefined && (missing0 = 'placement_policy')) ||
        (data.usage_policy === undefined && (missing0 = 'usage_policy')) ||
        (data.display_name === undefined && (missing0 = 'display_name')) ||
        (data.row_revision === undefined && (missing0 = 'row_revision')) ||
        (data.created_at === undefined && (missing0 = 'created_at')) ||
        (data.updated_at === undefined && (missing0 = 'updated_at'))
      ) {
        validate102.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!func7.call(schema40.properties, key0)) {
            validate102.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.allowed_origins !== undefined) {
            let data0 = data.allowed_origins;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  const _errs4 = errors;
                  if (typeof data0[i0] !== 'string') {
                    validate102.errors = [
                      {
                        instancePath: instancePath + '/allowed_origins/' + i0,
                        schemaPath: '#/properties/allowed_origins/items/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      },
                    ];
                    return false;
                  }
                  var valid1 = _errs4 === errors;
                  if (!valid1) {
                    break;
                  }
                }
              } else {
                validate102.errors = [
                  {
                    instancePath: instancePath + '/allowed_origins',
                    schemaPath: '#/properties/allowed_origins/type',
                    keyword: 'type',
                    params: { type: 'array' },
                    message: 'must be array',
                  },
                ];
                return false;
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.authenticated_owner !== undefined) {
              const _errs6 = errors;
              if (
                !validate103(data.authenticated_owner, {
                  instancePath: instancePath + '/authenticated_owner',
                  parentData: data,
                  parentDataProperty: 'authenticated_owner',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate103.errors : vErrors.concat(validate103.errors);
                errors = vErrors.length;
              }
              var valid0 = _errs6 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.created_at !== undefined) {
                const _errs7 = errors;
                if (
                  !validate105(data.created_at, {
                    instancePath: instancePath + '/created_at',
                    parentData: data,
                    parentDataProperty: 'created_at',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate105.errors : vErrors.concat(validate105.errors);
                  errors = vErrors.length;
                }
                var valid0 = _errs7 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.credential_id !== undefined) {
                  const _errs8 = errors;
                  if (
                    !validate107(data.credential_id, {
                      instancePath: instancePath + '/credential_id',
                      parentData: data,
                      parentDataProperty: 'credential_id',
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate107.errors : vErrors.concat(validate107.errors);
                    errors = vErrors.length;
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.credential_kind !== undefined) {
                    const _errs9 = errors;
                    if (
                      !validate109(data.credential_kind, {
                        instancePath: instancePath + '/credential_kind',
                        parentData: data,
                        parentDataProperty: 'credential_kind',
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate109.errors : vErrors.concat(validate109.errors);
                      errors = vErrors.length;
                    }
                    var valid0 = _errs9 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.credential_revision !== undefined) {
                      const _errs10 = errors;
                      if (
                        !validate111(data.credential_revision, {
                          instancePath: instancePath + '/credential_revision',
                          parentData: data,
                          parentDataProperty: 'credential_revision',
                          rootData,
                          dynamicAnchors,
                        })
                      ) {
                        vErrors =
                          vErrors === null
                            ? validate111.errors
                            : vErrors.concat(validate111.errors);
                        errors = vErrors.length;
                      }
                      var valid0 = _errs10 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.display_name !== undefined) {
                        const _errs11 = errors;
                        if (typeof data.display_name !== 'string') {
                          validate102.errors = [
                            {
                              instancePath: instancePath + '/display_name',
                              schemaPath: '#/properties/display_name/type',
                              keyword: 'type',
                              params: { type: 'string' },
                              message: 'must be string',
                            },
                          ];
                          return false;
                        }
                        var valid0 = _errs11 === errors;
                      } else {
                        var valid0 = true;
                      }
                      if (valid0) {
                        if (data.placement_policy !== undefined) {
                          const _errs13 = errors;
                          if (
                            !validate113(data.placement_policy, {
                              instancePath: instancePath + '/placement_policy',
                              parentData: data,
                              parentDataProperty: 'placement_policy',
                              rootData,
                              dynamicAnchors,
                            })
                          ) {
                            vErrors =
                              vErrors === null
                                ? validate113.errors
                                : vErrors.concat(validate113.errors);
                            errors = vErrors.length;
                          }
                          var valid0 = _errs13 === errors;
                        } else {
                          var valid0 = true;
                        }
                        if (valid0) {
                          if (data.row_revision !== undefined) {
                            const _errs14 = errors;
                            if (
                              !validate121(data.row_revision, {
                                instancePath: instancePath + '/row_revision',
                                parentData: data,
                                parentDataProperty: 'row_revision',
                                rootData,
                                dynamicAnchors,
                              })
                            ) {
                              vErrors =
                                vErrors === null
                                  ? validate121.errors
                                  : vErrors.concat(validate121.errors);
                              errors = vErrors.length;
                            }
                            var valid0 = _errs14 === errors;
                          } else {
                            var valid0 = true;
                          }
                          if (valid0) {
                            if (data.scope !== undefined) {
                              const _errs15 = errors;
                              if (
                                !validate123(data.scope, {
                                  instancePath: instancePath + '/scope',
                                  parentData: data,
                                  parentDataProperty: 'scope',
                                  rootData,
                                  dynamicAnchors,
                                })
                              ) {
                                vErrors =
                                  vErrors === null
                                    ? validate123.errors
                                    : vErrors.concat(validate123.errors);
                                errors = vErrors.length;
                              }
                              var valid0 = _errs15 === errors;
                            } else {
                              var valid0 = true;
                            }
                            if (valid0) {
                              if (data.state !== undefined) {
                                const _errs16 = errors;
                                if (
                                  !validate130(data.state, {
                                    instancePath: instancePath + '/state',
                                    parentData: data,
                                    parentDataProperty: 'state',
                                    rootData,
                                    dynamicAnchors,
                                  })
                                ) {
                                  vErrors =
                                    vErrors === null
                                      ? validate130.errors
                                      : vErrors.concat(validate130.errors);
                                  errors = vErrors.length;
                                }
                                var valid0 = _errs16 === errors;
                              } else {
                                var valid0 = true;
                              }
                              if (valid0) {
                                if (data.updated_at !== undefined) {
                                  const _errs17 = errors;
                                  if (
                                    !validate105(data.updated_at, {
                                      instancePath: instancePath + '/updated_at',
                                      parentData: data,
                                      parentDataProperty: 'updated_at',
                                      rootData,
                                      dynamicAnchors,
                                    })
                                  ) {
                                    vErrors =
                                      vErrors === null
                                        ? validate105.errors
                                        : vErrors.concat(validate105.errors);
                                    errors = vErrors.length;
                                  }
                                  var valid0 = _errs17 === errors;
                                } else {
                                  var valid0 = true;
                                }
                                if (valid0) {
                                  if (data.usage_policy !== undefined) {
                                    const _errs18 = errors;
                                    if (
                                      !validate133(data.usage_policy, {
                                        instancePath: instancePath + '/usage_policy',
                                        parentData: data,
                                        parentDataProperty: 'usage_policy',
                                        rootData,
                                        dynamicAnchors,
                                      })
                                    ) {
                                      vErrors =
                                        vErrors === null
                                          ? validate133.errors
                                          : vErrors.concat(validate133.errors);
                                      errors = vErrors.length;
                                    }
                                    var valid0 = _errs18 === errors;
                                  } else {
                                    var valid0 = true;
                                  }
                                }
                              }
                            }
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate102.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate102.errors = vErrors;
  return errors === 0;
}
validate102.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema58 = { enum: ['committed', 'aborted'], type: 'string' };
function validate140(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate140.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate140.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  if (!(data === 'committed' || data === 'aborted')) {
    validate140.errors = [
      {
        instancePath,
        schemaPath: '#/enum',
        keyword: 'enum',
        params: { allowedValues: schema58.enum },
        message: 'must be equal to one of the allowed values',
      },
    ];
    return false;
  }
  validate140.errors = vErrors;
  return errors === 0;
}
validate140.evaluated = { dynamicProps: false, dynamicItems: false };
function validate101(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate101.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.disposition === undefined && (missing0 = 'disposition')) ||
        (data.cleanup_pending === undefined && (missing0 = 'cleanup_pending'))
      ) {
        validate101.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(key0 === 'cleanup_pending' || key0 === 'credential' || key0 === 'disposition')) {
            validate101.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.cleanup_pending !== undefined) {
            const _errs2 = errors;
            if (typeof data.cleanup_pending !== 'boolean') {
              validate101.errors = [
                {
                  instancePath: instancePath + '/cleanup_pending',
                  schemaPath: '#/properties/cleanup_pending/type',
                  keyword: 'type',
                  params: { type: 'boolean' },
                  message: 'must be boolean',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.credential !== undefined) {
              let data1 = data.credential;
              const _errs4 = errors;
              const _errs5 = errors;
              let valid1 = false;
              const _errs6 = errors;
              if (
                !validate102(data1, {
                  instancePath: instancePath + '/credential',
                  parentData: data,
                  parentDataProperty: 'credential',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate102.errors : vErrors.concat(validate102.errors);
                errors = vErrors.length;
              }
              var _valid0 = _errs6 === errors;
              valid1 = valid1 || _valid0;
              const _errs7 = errors;
              if (data1 !== null) {
                const err0 = {
                  instancePath: instancePath + '/credential',
                  schemaPath: '#/properties/credential/anyOf/1/type',
                  keyword: 'type',
                  params: { type: 'null' },
                  message: 'must be null',
                };
                if (vErrors === null) {
                  vErrors = [err0];
                } else {
                  vErrors.push(err0);
                }
                errors++;
              }
              var _valid0 = _errs7 === errors;
              valid1 = valid1 || _valid0;
              if (!valid1) {
                const err1 = {
                  instancePath: instancePath + '/credential',
                  schemaPath: '#/properties/credential/anyOf',
                  keyword: 'anyOf',
                  params: {},
                  message: 'must match a schema in anyOf',
                };
                if (vErrors === null) {
                  vErrors = [err1];
                } else {
                  vErrors.push(err1);
                }
                errors++;
                validate101.errors = vErrors;
                return false;
              } else {
                errors = _errs5;
                if (vErrors !== null) {
                  if (_errs5) {
                    vErrors.length = _errs5;
                  } else {
                    vErrors = null;
                  }
                }
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.disposition !== undefined) {
                const _errs9 = errors;
                if (
                  !validate140(data.disposition, {
                    instancePath: instancePath + '/disposition',
                    parentData: data,
                    parentDataProperty: 'disposition',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate140.errors : vErrors.concat(validate140.errors);
                  errors = vErrors.length;
                }
                var valid0 = _errs9 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate101.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate101.errors = vErrors;
  return errors === 0;
}
validate101.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema59 = { enum: ['pending', 'committed', 'aborted', 'not_obtained'], type: 'string' };
function validate143(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate143.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate143.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  if (!(
    data === 'pending' ||
    data === 'committed' ||
    data === 'aborted' ||
    data === 'not_obtained'
  )) {
    validate143.errors = [
      {
        instancePath,
        schemaPath: '#/enum',
        keyword: 'enum',
        params: { allowedValues: schema59.enum },
        message: 'must be equal to one of the allowed values',
      },
    ];
    return false;
  }
  validate143.errors = vErrors;
  return errors === 0;
}
validate143.evaluated = { dynamicProps: false, dynamicItems: false };
function validate98(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate98.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.command_invocation_id === undefined && (missing0 = 'command_invocation_id')) ||
        (data.state === undefined && (missing0 = 'state'))
      ) {
        validate98.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(key0 === 'command_invocation_id' || key0 === 'result' || key0 === 'state')) {
            validate98.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.command_invocation_id !== undefined) {
            const _errs2 = errors;
            if (
              !validate99(data.command_invocation_id, {
                instancePath: instancePath + '/command_invocation_id',
                parentData: data,
                parentDataProperty: 'command_invocation_id',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate99.errors : vErrors.concat(validate99.errors);
              errors = vErrors.length;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.result !== undefined) {
              let data1 = data.result;
              const _errs3 = errors;
              const _errs4 = errors;
              let valid1 = false;
              const _errs5 = errors;
              if (
                !validate101(data1, {
                  instancePath: instancePath + '/result',
                  parentData: data,
                  parentDataProperty: 'result',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate101.errors : vErrors.concat(validate101.errors);
                errors = vErrors.length;
              }
              var _valid0 = _errs5 === errors;
              valid1 = valid1 || _valid0;
              const _errs6 = errors;
              if (data1 !== null) {
                const err0 = {
                  instancePath: instancePath + '/result',
                  schemaPath: '#/properties/result/anyOf/1/type',
                  keyword: 'type',
                  params: { type: 'null' },
                  message: 'must be null',
                };
                if (vErrors === null) {
                  vErrors = [err0];
                } else {
                  vErrors.push(err0);
                }
                errors++;
              }
              var _valid0 = _errs6 === errors;
              valid1 = valid1 || _valid0;
              if (!valid1) {
                const err1 = {
                  instancePath: instancePath + '/result',
                  schemaPath: '#/properties/result/anyOf',
                  keyword: 'anyOf',
                  params: {},
                  message: 'must match a schema in anyOf',
                };
                if (vErrors === null) {
                  vErrors = [err1];
                } else {
                  vErrors.push(err1);
                }
                errors++;
                validate98.errors = vErrors;
                return false;
              } else {
                errors = _errs4;
                if (vErrors !== null) {
                  if (_errs4) {
                    vErrors.length = _errs4;
                  } else {
                    vErrors = null;
                  }
                }
              }
              var valid0 = _errs3 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.state !== undefined) {
                const _errs8 = errors;
                if (
                  !validate143(data.state, {
                    instancePath: instancePath + '/state',
                    parentData: data,
                    parentDataProperty: 'state',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate143.errors : vErrors.concat(validate143.errors);
                  errors = vErrors.length;
                }
                var valid0 = _errs8 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate98.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate98.errors = vErrors;
  return errors === 0;
}
validate98.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema60 = {
  additionalProperties: false,
  properties: {
    credentials: { items: { $ref: '#/$defs/ToolCredentialAuthorityRecordV1' }, type: 'array' },
    pending_credential_ids: { items: { $ref: '#/$defs/CredentialId' }, type: 'array' },
  },
  required: ['credentials', 'pending_credential_ids'],
  type: 'object',
};
function validate151(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate151.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.credentials === undefined && (missing0 = 'credentials')) ||
        (data.pending_credential_ids === undefined && (missing0 = 'pending_credential_ids'))
      ) {
        validate151.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(key0 === 'credentials' || key0 === 'pending_credential_ids')) {
            validate151.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.credentials !== undefined) {
            let data0 = data.credentials;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  const _errs4 = errors;
                  if (
                    !validate102(data0[i0], {
                      instancePath: instancePath + '/credentials/' + i0,
                      parentData: data0,
                      parentDataProperty: i0,
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate102.errors : vErrors.concat(validate102.errors);
                    errors = vErrors.length;
                  }
                  var valid1 = _errs4 === errors;
                  if (!valid1) {
                    break;
                  }
                }
              } else {
                validate151.errors = [
                  {
                    instancePath: instancePath + '/credentials',
                    schemaPath: '#/properties/credentials/type',
                    keyword: 'type',
                    params: { type: 'array' },
                    message: 'must be array',
                  },
                ];
                return false;
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.pending_credential_ids !== undefined) {
              let data2 = data.pending_credential_ids;
              const _errs5 = errors;
              if (errors === _errs5) {
                if (Array.isArray(data2)) {
                  var valid2 = true;
                  const len1 = data2.length;
                  for (let i1 = 0; i1 < len1; i1++) {
                    const _errs7 = errors;
                    if (
                      !validate107(data2[i1], {
                        instancePath: instancePath + '/pending_credential_ids/' + i1,
                        parentData: data2,
                        parentDataProperty: i1,
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate107.errors : vErrors.concat(validate107.errors);
                      errors = vErrors.length;
                    }
                    var valid2 = _errs7 === errors;
                    if (!valid2) {
                      break;
                    }
                  }
                } else {
                  validate151.errors = [
                    {
                      instancePath: instancePath + '/pending_credential_ids',
                      schemaPath: '#/properties/pending_credential_ids/type',
                      keyword: 'type',
                      params: { type: 'array' },
                      message: 'must be array',
                    },
                  ];
                  return false;
                }
              }
              var valid0 = _errs5 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate151.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate151.errors = vErrors;
  return errors === 0;
}
validate151.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate97(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate97.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs0 = errors;
  let valid0 = false;
  const _errs1 = errors;
  if (errors === _errs1) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.command === undefined && (missing0 = 'command')) ||
        (data.value === undefined && (missing0 = 'value'))
      ) {
        const err0 = {
          instancePath,
          schemaPath: '#/anyOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs3 = errors;
        for (const key0 in data) {
          if (!(key0 === 'command' || key0 === 'value')) {
            const err1 = {
              instancePath,
              schemaPath: '#/anyOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs3 === errors) {
          if (data.command !== undefined) {
            const _errs4 = errors;
            if ('tools.credential.outcome' !== data.command) {
              const err2 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/0/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.outcome' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err2];
              } else {
                vErrors.push(err2);
              }
              errors++;
            }
            var valid1 = _errs4 === errors;
          } else {
            var valid1 = true;
          }
          if (valid1) {
            if (data.value !== undefined) {
              const _errs5 = errors;
              if (
                !validate98(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate98.errors : vErrors.concat(validate98.errors);
                errors = vErrors.length;
              }
              var valid1 = _errs5 === errors;
            } else {
              var valid1 = true;
            }
          }
        }
      }
    } else {
      const err3 = {
        instancePath,
        schemaPath: '#/anyOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err3];
      } else {
        vErrors.push(err3);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    var props0 = true;
  }
  const _errs6 = errors;
  if (errors === _errs6) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (
        (data.command === undefined && (missing1 = 'command')) ||
        (data.value === undefined && (missing1 = 'value'))
      ) {
        const err4 = {
          instancePath,
          schemaPath: '#/anyOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err4];
        } else {
          vErrors.push(err4);
        }
        errors++;
      } else {
        const _errs8 = errors;
        for (const key1 in data) {
          if (!(key1 === 'command' || key1 === 'value')) {
            const err5 = {
              instancePath,
              schemaPath: '#/anyOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err5];
            } else {
              vErrors.push(err5);
            }
            errors++;
            break;
          }
        }
        if (_errs8 === errors) {
          if (data.command !== undefined) {
            const _errs9 = errors;
            if ('tools.credential.create' !== data.command) {
              const err6 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/1/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.create' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err6];
              } else {
                vErrors.push(err6);
              }
              errors++;
            }
            var valid2 = _errs9 === errors;
          } else {
            var valid2 = true;
          }
          if (valid2) {
            if (data.value !== undefined) {
              const _errs10 = errors;
              if (
                !validate101(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate101.errors : vErrors.concat(validate101.errors);
                errors = vErrors.length;
              }
              var valid2 = _errs10 === errors;
            } else {
              var valid2 = true;
            }
          }
        }
      }
    } else {
      const err7 = {
        instancePath,
        schemaPath: '#/anyOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err7];
      } else {
        vErrors.push(err7);
      }
      errors++;
    }
  }
  var _valid0 = _errs6 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs11 = errors;
  if (errors === _errs11) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing2;
      if (
        (data.command === undefined && (missing2 = 'command')) ||
        (data.value === undefined && (missing2 = 'value'))
      ) {
        const err8 = {
          instancePath,
          schemaPath: '#/anyOf/2/required',
          keyword: 'required',
          params: { missingProperty: missing2 },
          message: "must have required property '" + missing2 + "'",
        };
        if (vErrors === null) {
          vErrors = [err8];
        } else {
          vErrors.push(err8);
        }
        errors++;
      } else {
        const _errs13 = errors;
        for (const key2 in data) {
          if (!(key2 === 'command' || key2 === 'value')) {
            const err9 = {
              instancePath,
              schemaPath: '#/anyOf/2/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key2 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err9];
            } else {
              vErrors.push(err9);
            }
            errors++;
            break;
          }
        }
        if (_errs13 === errors) {
          if (data.command !== undefined) {
            const _errs14 = errors;
            if ('tools.credential.rotate' !== data.command) {
              const err10 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/2/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.rotate' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err10];
              } else {
                vErrors.push(err10);
              }
              errors++;
            }
            var valid3 = _errs14 === errors;
          } else {
            var valid3 = true;
          }
          if (valid3) {
            if (data.value !== undefined) {
              const _errs15 = errors;
              if (
                !validate101(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate101.errors : vErrors.concat(validate101.errors);
                errors = vErrors.length;
              }
              var valid3 = _errs15 === errors;
            } else {
              var valid3 = true;
            }
          }
        }
      }
    } else {
      const err11 = {
        instancePath,
        schemaPath: '#/anyOf/2/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err11];
      } else {
        vErrors.push(err11);
      }
      errors++;
    }
  }
  var _valid0 = _errs11 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs16 = errors;
  if (errors === _errs16) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing3;
      if (
        (data.command === undefined && (missing3 = 'command')) ||
        (data.value === undefined && (missing3 = 'value'))
      ) {
        const err12 = {
          instancePath,
          schemaPath: '#/anyOf/3/required',
          keyword: 'required',
          params: { missingProperty: missing3 },
          message: "must have required property '" + missing3 + "'",
        };
        if (vErrors === null) {
          vErrors = [err12];
        } else {
          vErrors.push(err12);
        }
        errors++;
      } else {
        const _errs18 = errors;
        for (const key3 in data) {
          if (!(key3 === 'command' || key3 === 'value')) {
            const err13 = {
              instancePath,
              schemaPath: '#/anyOf/3/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key3 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err13];
            } else {
              vErrors.push(err13);
            }
            errors++;
            break;
          }
        }
        if (_errs18 === errors) {
          if (data.command !== undefined) {
            const _errs19 = errors;
            if ('tools.credential.clear' !== data.command) {
              const err14 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/3/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.clear' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err14];
              } else {
                vErrors.push(err14);
              }
              errors++;
            }
            var valid4 = _errs19 === errors;
          } else {
            var valid4 = true;
          }
          if (valid4) {
            if (data.value !== undefined) {
              const _errs20 = errors;
              if (
                !validate101(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate101.errors : vErrors.concat(validate101.errors);
                errors = vErrors.length;
              }
              var valid4 = _errs20 === errors;
            } else {
              var valid4 = true;
            }
          }
        }
      }
    } else {
      const err15 = {
        instancePath,
        schemaPath: '#/anyOf/3/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err15];
      } else {
        vErrors.push(err15);
      }
      errors++;
    }
  }
  var _valid0 = _errs16 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs21 = errors;
  if (errors === _errs21) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing4;
      if (
        (data.command === undefined && (missing4 = 'command')) ||
        (data.value === undefined && (missing4 = 'value'))
      ) {
        const err16 = {
          instancePath,
          schemaPath: '#/anyOf/4/required',
          keyword: 'required',
          params: { missingProperty: missing4 },
          message: "must have required property '" + missing4 + "'",
        };
        if (vErrors === null) {
          vErrors = [err16];
        } else {
          vErrors.push(err16);
        }
        errors++;
      } else {
        const _errs23 = errors;
        for (const key4 in data) {
          if (!(key4 === 'command' || key4 === 'value')) {
            const err17 = {
              instancePath,
              schemaPath: '#/anyOf/4/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key4 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err17];
            } else {
              vErrors.push(err17);
            }
            errors++;
            break;
          }
        }
        if (_errs23 === errors) {
          if (data.command !== undefined) {
            const _errs24 = errors;
            if ('tools.credential.origins' !== data.command) {
              const err18 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/4/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.origins' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err18];
              } else {
                vErrors.push(err18);
              }
              errors++;
            }
            var valid5 = _errs24 === errors;
          } else {
            var valid5 = true;
          }
          if (valid5) {
            if (data.value !== undefined) {
              const _errs25 = errors;
              if (
                !validate101(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate101.errors : vErrors.concat(validate101.errors);
                errors = vErrors.length;
              }
              var valid5 = _errs25 === errors;
            } else {
              var valid5 = true;
            }
          }
        }
      }
    } else {
      const err19 = {
        instancePath,
        schemaPath: '#/anyOf/4/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err19];
      } else {
        vErrors.push(err19);
      }
      errors++;
    }
  }
  var _valid0 = _errs21 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs26 = errors;
  if (errors === _errs26) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing5;
      if (
        (data.command === undefined && (missing5 = 'command')) ||
        (data.value === undefined && (missing5 = 'value'))
      ) {
        const err20 = {
          instancePath,
          schemaPath: '#/anyOf/5/required',
          keyword: 'required',
          params: { missingProperty: missing5 },
          message: "must have required property '" + missing5 + "'",
        };
        if (vErrors === null) {
          vErrors = [err20];
        } else {
          vErrors.push(err20);
        }
        errors++;
      } else {
        const _errs28 = errors;
        for (const key5 in data) {
          if (!(key5 === 'command' || key5 === 'value')) {
            const err21 = {
              instancePath,
              schemaPath: '#/anyOf/5/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key5 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err21];
            } else {
              vErrors.push(err21);
            }
            errors++;
            break;
          }
        }
        if (_errs28 === errors) {
          if (data.command !== undefined) {
            const _errs29 = errors;
            if ('tools.credential.recover' !== data.command) {
              const err22 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/5/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.recover' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err22];
              } else {
                vErrors.push(err22);
              }
              errors++;
            }
            var valid6 = _errs29 === errors;
          } else {
            var valid6 = true;
          }
          if (valid6) {
            if (data.value !== undefined) {
              const _errs30 = errors;
              if (
                !validate101(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate101.errors : vErrors.concat(validate101.errors);
                errors = vErrors.length;
              }
              var valid6 = _errs30 === errors;
            } else {
              var valid6 = true;
            }
          }
        }
      }
    } else {
      const err23 = {
        instancePath,
        schemaPath: '#/anyOf/5/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err23];
      } else {
        vErrors.push(err23);
      }
      errors++;
    }
  }
  var _valid0 = _errs26 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs31 = errors;
  if (errors === _errs31) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing6;
      if (
        (data.command === undefined && (missing6 = 'command')) ||
        (data.value === undefined && (missing6 = 'value'))
      ) {
        const err24 = {
          instancePath,
          schemaPath: '#/anyOf/6/required',
          keyword: 'required',
          params: { missingProperty: missing6 },
          message: "must have required property '" + missing6 + "'",
        };
        if (vErrors === null) {
          vErrors = [err24];
        } else {
          vErrors.push(err24);
        }
        errors++;
      } else {
        const _errs33 = errors;
        for (const key6 in data) {
          if (!(key6 === 'command' || key6 === 'value')) {
            const err25 = {
              instancePath,
              schemaPath: '#/anyOf/6/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key6 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err25];
            } else {
              vErrors.push(err25);
            }
            errors++;
            break;
          }
        }
        if (_errs33 === errors) {
          if (data.command !== undefined) {
            const _errs34 = errors;
            if ('tools.credential.list' !== data.command) {
              const err26 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/6/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'tools.credential.list' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err26];
              } else {
                vErrors.push(err26);
              }
              errors++;
            }
            var valid7 = _errs34 === errors;
          } else {
            var valid7 = true;
          }
          if (valid7) {
            if (data.value !== undefined) {
              const _errs35 = errors;
              if (
                !validate151(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate151.errors : vErrors.concat(validate151.errors);
                errors = vErrors.length;
              }
              var valid7 = _errs35 === errors;
            } else {
              var valid7 = true;
            }
          }
        }
      }
    } else {
      const err27 = {
        instancePath,
        schemaPath: '#/anyOf/6/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err27];
      } else {
        vErrors.push(err27);
      }
      errors++;
    }
  }
  var _valid0 = _errs31 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  if (!valid0) {
    const err28 = {
      instancePath,
      schemaPath: '#/anyOf',
      keyword: 'anyOf',
      params: {},
      message: 'must match a schema in anyOf',
    };
    if (vErrors === null) {
      vErrors = [err28];
    } else {
      vErrors.push(err28);
    }
    errors++;
    validate97.errors = vErrors;
    return false;
  } else {
    errors = _errs0;
    if (vErrors !== null) {
      if (_errs0) {
        vErrors.length = _errs0;
      } else {
        vErrors = null;
      }
    }
  }
  validate97.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate97.evaluated = { dynamicProps: true, dynamicItems: false };
