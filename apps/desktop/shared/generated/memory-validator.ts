// @ts-nocheck
/* eslint-disable */
/** Generated from Rust route/schema facts using AJV standalone. Do not edit. */
/* AJV runtime helpers, and fast-deep-equal (Copyright (c) 2017 Evgeny Poberezkin).
The MIT License (MIT)

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
export const validateMemoryRequest = validate52;
const schema19 = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  anyOf: [
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.timeline' },
        value: { $ref: '#/$defs/MemoryTimelineRequest' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.read_many' },
        value: { $ref: '#/$defs/MemoryBatchReadRequest' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vaults.get' },
        value: { $ref: '#/$defs/MemoryDesktopRequest' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.settings.get' },
        value: { $ref: '#/$defs/EmptyToolInput' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.settings.update' },
        value: { $ref: '#/$defs/MemorySettingsUpdate' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.validate' },
        value: { $ref: '#/$defs/MemoryVaultRequest' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.rebuild' },
        value: { $ref: '#/$defs/MemoryVaultRequest' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.repair.preview' },
        value: { $ref: '#/$defs/MemoryVaultRequest' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.repair.apply' },
        value: { $ref: '#/$defs/MemoryDesktopRepairApply' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.open_directory' },
        value: { $ref: '#/$defs/MemoryVaultRequest' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.maintenance.open_log' },
        value: { $ref: '#/$defs/MemoryVaultRequest' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.export' },
        value: { $ref: '#/$defs/MemoryVaultRequest' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.export.prepare' },
        value: { $ref: '#/$defs/MemoryVaultRequest' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.export.entry' },
        value: { $ref: '#/$defs/MemoryExportEntryRequest' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.export.chunk' },
        value: { $ref: '#/$defs/MemoryExportChunkRequest' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.export.close' },
        value: { $ref: '#/$defs/MemoryExportCloseRequest' },
      },
    },
  ],
  $defs: {
    MemoryTimelineRequest: {
      additionalProperties: false,
      properties: {
        query: { $ref: '#/$defs/MemoryTimelineQuery' },
        vault: { $ref: '#/$defs/MemoryVaultRequest' },
      },
      required: ['vault', 'query'],
      type: 'object',
    },
    MemoryTimelineQuery: {
      additionalProperties: false,
      properties: {
        author_session_id: { anyOf: [{ $ref: '#/$defs/SessionId' }, { type: 'null' }] },
        before: {
          description: 'The record hash, not a row number tied to one SQLite rebuild.',
          type: ['string', 'null'],
        },
        category: {
          description: 'Current category of the note; deleted/unindexed notes have no category.',
          type: ['string', 'null'],
        },
        limit: { maximum: 100, minimum: 1, type: ['integer', 'null'] },
        since: { anyOf: [{ $ref: '#/$defs/UtcTimestamp' }, { type: 'null' }] },
        until: { anyOf: [{ $ref: '#/$defs/UtcTimestamp' }, { type: 'null' }] },
      },
      type: 'object',
    },
    SessionId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    UtcTimestamp: { type: 'string' },
    MemoryVaultRequest: {
      additionalProperties: false,
      properties: {
        scope: { $ref: '#/$defs/MemoryScope' },
        workspace_id: { anyOf: [{ $ref: '#/$defs/WorkspaceId' }, { type: 'null' }] },
      },
      required: ['scope'],
      type: 'object',
    },
    MemoryScope: { enum: ['global', 'project'], type: 'string' },
    WorkspaceId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^wsp_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    MemoryTimeline: {
      additionalProperties: false,
      properties: {
        before: { type: ['string', 'null'] },
        entries: { items: { $ref: '#/$defs/MemoryTimelineEntry' }, type: 'array' },
        incomplete: { type: 'boolean' },
      },
      required: ['entries', 'incomplete'],
      type: 'object',
    },
    MemoryTimelineEntry: {
      additionalProperties: false,
      properties: {
        category: { type: ['string', 'null'] },
        cursor: { type: 'string' },
        event: { $ref: '#/$defs/MemoryHistory' },
        session_name_at_write: { type: ['string', 'null'] },
      },
      required: ['cursor', 'event'],
      type: 'object',
    },
    MemoryHistory: {
      additionalProperties: false,
      properties: {
        action: { type: 'string' },
        actor: { type: 'string' },
        after_hash: { type: ['string', 'null'] },
        at: { $ref: '#/$defs/UtcTimestamp' },
        before_hash: { type: ['string', 'null'] },
        id: { anyOf: [{ $ref: '#/$defs/MemoryId' }, { type: 'null' }] },
        path: { type: 'string' },
        session_id: { anyOf: [{ $ref: '#/$defs/SessionId' }, { type: 'null' }] },
        title: { type: 'string' },
        turn_id: { type: ['string', 'null'] },
      },
      required: ['path', 'action', 'actor', 'title', 'at'],
      type: 'object',
    },
    MemoryId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^mem_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    MemoryBatchReadRequest: {
      additionalProperties: false,
      properties: {
        ids: { items: { $ref: '#/$defs/MemoryId' }, maxItems: 100, minItems: 1, type: 'array' },
        vault: { $ref: '#/$defs/MemoryVaultRequest' },
      },
      required: ['vault', 'ids'],
      type: 'object',
    },
    MemoryBatchRead: {
      additionalProperties: false,
      properties: {
        missing_ids: { items: { $ref: '#/$defs/MemoryId' }, type: 'array' },
        notes: { items: { $ref: '#/$defs/MemoryRead' }, type: 'array' },
      },
      required: ['notes', 'missing_ids'],
      type: 'object',
    },
    MemoryRead: {
      additionalProperties: false,
      properties: {
        hash: { type: 'string' },
        history: { items: { $ref: '#/$defs/MemoryHistory' }, type: 'array' },
        note: { $ref: '#/$defs/MemoryDocument' },
        path: { type: 'string' },
        provenance: { anyOf: [{ $ref: '#/$defs/MemoryProvenance' }, { type: 'null' }] },
      },
      required: ['path', 'note', 'hash', 'history'],
      type: 'object',
    },
    MemoryDocument: {
      additionalProperties: false,
      properties: { body: { type: 'string' }, metadata: { $ref: '#/$defs/MemoryMetadata' } },
      required: ['metadata', 'body'],
      type: 'object',
    },
    MemoryMetadata: {
      additionalProperties: false,
      properties: {
        category: { type: 'string' },
        confidence: { type: 'number' },
        conflicts_with_ids: { items: { $ref: '#/$defs/MemoryId' }, type: 'array' },
        created_at: { $ref: '#/$defs/UtcTimestamp' },
        id: { $ref: '#/$defs/MemoryId' },
        importance: { maximum: 255, minimum: 0, type: 'integer' },
        last_confirmed_at: { anyOf: [{ $ref: '#/$defs/UtcTimestamp' }, { type: 'null' }] },
        recall_weight: { type: 'number' },
        related_ids: { items: { $ref: '#/$defs/MemoryId' }, type: 'array' },
        schema_version: { minimum: 0, type: 'integer', maximum: 4294967295 },
        scope: { $ref: '#/$defs/MemoryScope' },
        source_events: { items: { type: 'string' }, type: 'array' },
        source_sessions: { items: { $ref: '#/$defs/SessionId' }, type: 'array' },
        status: { $ref: '#/$defs/MemoryStatus' },
        supersedes_ids: { items: { $ref: '#/$defs/MemoryId' }, type: 'array' },
        tags: { items: { type: 'string' }, type: 'array' },
        updated_at: { $ref: '#/$defs/UtcTimestamp' },
      },
      required: [
        'schema_version',
        'id',
        'scope',
        'category',
        'status',
        'confidence',
        'importance',
        'recall_weight',
        'created_at',
        'updated_at',
        'source_sessions',
        'source_events',
        'tags',
        'related_ids',
        'supersedes_ids',
        'conflicts_with_ids',
      ],
      type: 'object',
    },
    MemoryStatus: {
      enum: ['active', 'superseded', 'forgotten', 'conflicted', 'archived'],
      type: 'string',
    },
    MemoryProvenance: {
      additionalProperties: false,
      properties: {
        creator: {
          anyOf: [{ $ref: '#/$defs/MemoryAuthorStamp' }, { type: 'null' }],
          description:
            'None means there is no reliable creation record. Source sessions alone are not proof.',
        },
        modifiers: {
          description: 'New authorship stamps plus recent known events, most recent last.',
          items: { $ref: '#/$defs/MemoryAuthorStamp' },
          type: 'array',
        },
      },
      required: ['modifiers'],
      type: 'object',
    },
    MemoryAuthorStamp: {
      additionalProperties: false,
      properties: {
        actor: { type: 'string' },
        at: { $ref: '#/$defs/UtcTimestamp' },
        session_id: { anyOf: [{ $ref: '#/$defs/SessionId' }, { type: 'null' }] },
        session_name_at_write: { type: ['string', 'null'] },
      },
      required: ['actor', 'at'],
      type: 'object',
    },
    MemoryDesktopRequest: {
      additionalProperties: false,
      properties: { workspace_id: { anyOf: [{ $ref: '#/$defs/WorkspaceId' }, { type: 'null' }] } },
      type: 'object',
    },
    MemoryDesktopSnapshot: {
      additionalProperties: false,
      properties: {
        recent_runs: { items: { $ref: '#/$defs/MemoryMaintenanceRun' }, type: 'array' },
        vaults: { items: { $ref: '#/$defs/MemoryVaultStatus' }, type: 'array' },
      },
      required: ['vaults', 'recent_runs'],
      type: 'object',
    },
    MemoryMaintenanceRun: {
      additionalProperties: false,
      properties: {
        finished_at: { anyOf: [{ $ref: '#/$defs/UtcTimestamp' }, { type: 'null' }] },
        input_tokens: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        model_profile_id: { anyOf: [{ $ref: '#/$defs/ModelProfileId' }, { type: 'null' }] },
        output_tokens: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        reason: { type: ['string', 'null'] },
        run_id: { type: 'string' },
        runtime_instance_id: { $ref: '#/$defs/RuntimeInstanceId' },
        session_id: { $ref: '#/$defs/SessionId' },
        started_at: { $ref: '#/$defs/UtcTimestamp' },
        state: { $ref: '#/$defs/MemoryMaintenanceState' },
        turn_id: { type: ['string', 'null'] },
        usage_complete: {
          default: false,
          description: 'Counts observed so far; false means final Provider usage is unconfirmed.',
          type: 'boolean',
        },
      },
      required: [
        'run_id',
        'runtime_instance_id',
        'session_id',
        'state',
        'input_tokens',
        'output_tokens',
        'started_at',
      ],
      type: 'object',
    },
    ModelProfileId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^mdl_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    RuntimeInstanceId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^run_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    MemoryMaintenanceState: {
      enum: ['queued', 'running', 'completed', 'skipped', 'failed', 'interrupted'],
      type: 'string',
    },
    MemoryVaultStatus: {
      additionalProperties: false,
      properties: {
        directory: { type: 'string' },
        exists: { type: 'boolean' },
        invalid: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        maintenance_log: {
          default: null,
          description:
            "The plain text log of this Vault's finished maintenance runs, once one was written.",
          type: ['string', 'null'],
        },
        rebuilding: { type: 'boolean' },
        recent_changes: { items: { $ref: '#/$defs/MemoryHistory' }, type: 'array' },
        scope: { $ref: '#/$defs/MemoryScope' },
        valid: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        watcher_error: { type: ['string', 'null'] },
      },
      required: [
        'scope',
        'directory',
        'exists',
        'valid',
        'invalid',
        'rebuilding',
        'recent_changes',
      ],
      type: 'object',
    },
    EmptyToolInput: { additionalProperties: false, type: 'object' },
    MemorySettingsSnapshot: {
      additionalProperties: false,
      properties: {
        issue: { type: ['string', 'null'] },
        revision: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
        settings: { $ref: '#/$defs/MemorySettings' },
      },
      required: ['settings'],
      type: 'object',
    },
    MemorySettings: {
      additionalProperties: false,
      properties: {
        automatic_maintenance: { type: 'boolean' },
        event_retention_days: { maximum: 3650, minimum: 1, type: 'integer' },
      },
      required: ['automatic_maintenance', 'event_retention_days'],
      type: 'object',
    },
    MemorySettingsUpdate: {
      additionalProperties: false,
      properties: {
        expected_revision: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        settings: { $ref: '#/$defs/MemorySettings' },
      },
      required: ['expected_revision', 'settings'],
      type: 'object',
    },
    MemoryValidation: {
      additionalProperties: false,
      properties: {
        invalid: { items: { $ref: '#/$defs/MemoryInvalidFile' }, type: 'array' },
        rebuilding: { type: 'boolean' },
        valid: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
      },
      required: ['valid', 'invalid', 'rebuilding'],
      type: 'object',
    },
    MemoryInvalidFile: {
      additionalProperties: false,
      properties: { path: { type: 'string' }, reason: { type: 'string' } },
      required: ['path', 'reason'],
      type: 'object',
    },
    MemoryRepairPreview: {
      additionalProperties: false,
      properties: {
        changes: { items: { $ref: '#/$defs/MemoryRepairChange' }, type: 'array' },
        manual: { items: { $ref: '#/$defs/MemoryInvalidFile' }, type: 'array' },
        preview_token: { type: 'string' },
        rebuild_index: { type: 'boolean' },
      },
      required: ['preview_token', 'changes', 'manual', 'rebuild_index'],
      type: 'object',
    },
    MemoryRepairChange: {
      additionalProperties: false,
      properties: {
        changes: { items: { type: 'string' }, type: 'array' },
        expected_hash: { type: ['string', 'null'] },
        path: { type: 'string' },
        proposed_hash: { type: 'string' },
      },
      required: ['path', 'proposed_hash', 'changes'],
      type: 'object',
    },
    MemoryDesktopRepairApply: {
      additionalProperties: false,
      properties: { preview_id: { type: 'string' }, vault: { $ref: '#/$defs/MemoryVaultRequest' } },
      required: ['vault', 'preview_id'],
      type: 'object',
    },
    MemoryRepairResult: {
      additionalProperties: false,
      properties: {
        applied: { type: 'boolean' },
        preview: { $ref: '#/$defs/MemoryRepairPreview' },
      },
      required: ['preview', 'applied'],
      type: 'object',
    },
    MemoryDesktopActionResult: {
      additionalProperties: false,
      properties: { completed: { type: 'boolean' }, message: { type: 'string' } },
      required: ['completed', 'message'],
      type: 'object',
    },
    MemoryExportHandle: {
      additionalProperties: false,
      properties: {
        entries: { minimum: 0, type: 'integer', maximum: 4294967295 },
        export_id: { type: 'string' },
        total_bytes: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
      },
      required: ['export_id', 'entries', 'total_bytes'],
      type: 'object',
    },
    MemoryExportEntryRequest: {
      additionalProperties: false,
      properties: {
        export_id: { type: 'string' },
        index: { minimum: 0, type: 'integer', maximum: 4294967295 },
      },
      required: ['export_id', 'index'],
      type: 'object',
    },
    MemoryExportEntry: {
      additionalProperties: false,
      properties: {
        directory: { type: 'boolean' },
        length: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        path: { type: 'string' },
        sha256: { type: 'string' },
      },
      required: ['path', 'directory', 'length', 'sha256'],
      type: 'object',
    },
    MemoryExportChunkRequest: {
      additionalProperties: false,
      properties: {
        export_id: { type: 'string' },
        index: { minimum: 0, type: 'integer', maximum: 4294967295 },
        max_bytes: { maximum: 65536, minimum: 1, type: 'integer' },
        offset: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
      },
      required: ['export_id', 'index', 'offset', 'max_bytes'],
      type: 'object',
    },
    MemoryExportChunk: {
      additionalProperties: false,
      properties: {
        bytes: { items: { maximum: 255, minimum: 0, type: 'integer' }, type: 'array' },
        eof: { type: 'boolean' },
        next_offset: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        offset: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
      },
      required: ['offset', 'next_offset', 'bytes', 'eof'],
      type: 'object',
    },
    MemoryExportCloseRequest: {
      additionalProperties: false,
      properties: { export_id: { type: 'string' } },
      required: ['export_id'],
      type: 'object',
    },
  },
};
const schema20 = {
  additionalProperties: false,
  properties: {
    query: { $ref: '#/$defs/MemoryTimelineQuery' },
    vault: { $ref: '#/$defs/MemoryVaultRequest' },
  },
  required: ['vault', 'query'],
  type: 'object',
};
const schema21 = {
  additionalProperties: false,
  properties: {
    author_session_id: { anyOf: [{ $ref: '#/$defs/SessionId' }, { type: 'null' }] },
    before: {
      description: 'The record hash, not a row number tied to one SQLite rebuild.',
      type: ['string', 'null'],
    },
    category: {
      description: 'Current category of the note; deleted/unindexed notes have no category.',
      type: ['string', 'null'],
    },
    limit: { maximum: 100, minimum: 1, type: ['integer', 'null'] },
    since: { anyOf: [{ $ref: '#/$defs/UtcTimestamp' }, { type: 'null' }] },
    until: { anyOf: [{ $ref: '#/$defs/UtcTimestamp' }, { type: 'null' }] },
  },
  type: 'object',
};
const schema22 = {
  type: 'string',
  minLength: 30,
  maxLength: 30,
  pattern: '^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
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
const pattern3 = new RegExp('^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$', 'u');
function validate55(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate55.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 30) {
        validate55.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 30 },
            message: 'must NOT have more than 30 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 30) {
          validate55.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 30 },
              message: 'must NOT have fewer than 30 characters',
            },
          ];
          return false;
        } else {
          if (!pattern3.test(data)) {
            validate55.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: '^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$' },
                message: 'must match pattern "' + '^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$' + '"',
              },
            ];
            return false;
          }
        }
      }
    } else {
      validate55.errors = [
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
  validate55.errors = vErrors;
  return errors === 0;
}
validate55.evaluated = { dynamicProps: false, dynamicItems: false };
const schema23 = { type: 'string' };
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
  if (typeof data !== 'string') {
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
  validate57.errors = vErrors;
  return errors === 0;
}
validate57.evaluated = { dynamicProps: false, dynamicItems: false };
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
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      const _errs1 = errors;
      for (const key0 in data) {
        if (!(
          key0 === 'author_session_id' ||
          key0 === 'before' ||
          key0 === 'category' ||
          key0 === 'limit' ||
          key0 === 'since' ||
          key0 === 'until'
        )) {
          validate54.errors = [
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
        if (data.author_session_id !== undefined) {
          let data0 = data.author_session_id;
          const _errs2 = errors;
          const _errs3 = errors;
          let valid1 = false;
          const _errs4 = errors;
          if (
            !validate55(data0, {
              instancePath: instancePath + '/author_session_id',
              parentData: data,
              parentDataProperty: 'author_session_id',
              rootData,
              dynamicAnchors,
            })
          ) {
            vErrors = vErrors === null ? validate55.errors : vErrors.concat(validate55.errors);
            errors = vErrors.length;
          }
          var _valid0 = _errs4 === errors;
          valid1 = valid1 || _valid0;
          const _errs5 = errors;
          if (data0 !== null) {
            const err0 = {
              instancePath: instancePath + '/author_session_id',
              schemaPath: '#/properties/author_session_id/anyOf/1/type',
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
          var _valid0 = _errs5 === errors;
          valid1 = valid1 || _valid0;
          if (!valid1) {
            const err1 = {
              instancePath: instancePath + '/author_session_id',
              schemaPath: '#/properties/author_session_id/anyOf',
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
            validate54.errors = vErrors;
            return false;
          } else {
            errors = _errs3;
            if (vErrors !== null) {
              if (_errs3) {
                vErrors.length = _errs3;
              } else {
                vErrors = null;
              }
            }
          }
          var valid0 = _errs2 === errors;
        } else {
          var valid0 = true;
        }
        if (valid0) {
          if (data.before !== undefined) {
            let data1 = data.before;
            const _errs7 = errors;
            if (typeof data1 !== 'string' && data1 !== null) {
              validate54.errors = [
                {
                  instancePath: instancePath + '/before',
                  schemaPath: '#/properties/before/type',
                  keyword: 'type',
                  params: { type: schema21.properties.before.type },
                  message: 'must be string,null',
                },
              ];
              return false;
            }
            var valid0 = _errs7 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.category !== undefined) {
              let data2 = data.category;
              const _errs9 = errors;
              if (typeof data2 !== 'string' && data2 !== null) {
                validate54.errors = [
                  {
                    instancePath: instancePath + '/category',
                    schemaPath: '#/properties/category/type',
                    keyword: 'type',
                    params: { type: schema21.properties.category.type },
                    message: 'must be string,null',
                  },
                ];
                return false;
              }
              var valid0 = _errs9 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.limit !== undefined) {
                let data3 = data.limit;
                const _errs11 = errors;
                if (
                  !(typeof data3 == 'number' && !(data3 % 1) && !isNaN(data3) && isFinite(data3)) &&
                  data3 !== null
                ) {
                  validate54.errors = [
                    {
                      instancePath: instancePath + '/limit',
                      schemaPath: '#/properties/limit/type',
                      keyword: 'type',
                      params: { type: schema21.properties.limit.type },
                      message: 'must be integer,null',
                    },
                  ];
                  return false;
                }
                if (errors === _errs11) {
                  if (typeof data3 == 'number' && isFinite(data3)) {
                    if (data3 > 100 || isNaN(data3)) {
                      validate54.errors = [
                        {
                          instancePath: instancePath + '/limit',
                          schemaPath: '#/properties/limit/maximum',
                          keyword: 'maximum',
                          params: { comparison: '<=', limit: 100 },
                          message: 'must be <= 100',
                        },
                      ];
                      return false;
                    } else {
                      if (data3 < 1 || isNaN(data3)) {
                        validate54.errors = [
                          {
                            instancePath: instancePath + '/limit',
                            schemaPath: '#/properties/limit/minimum',
                            keyword: 'minimum',
                            params: { comparison: '>=', limit: 1 },
                            message: 'must be >= 1',
                          },
                        ];
                        return false;
                      }
                    }
                  }
                }
                var valid0 = _errs11 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.since !== undefined) {
                  let data4 = data.since;
                  const _errs13 = errors;
                  const _errs14 = errors;
                  let valid2 = false;
                  const _errs15 = errors;
                  if (
                    !validate57(data4, {
                      instancePath: instancePath + '/since',
                      parentData: data,
                      parentDataProperty: 'since',
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate57.errors : vErrors.concat(validate57.errors);
                    errors = vErrors.length;
                  }
                  var _valid1 = _errs15 === errors;
                  valid2 = valid2 || _valid1;
                  const _errs16 = errors;
                  if (data4 !== null) {
                    const err2 = {
                      instancePath: instancePath + '/since',
                      schemaPath: '#/properties/since/anyOf/1/type',
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
                  var _valid1 = _errs16 === errors;
                  valid2 = valid2 || _valid1;
                  if (!valid2) {
                    const err3 = {
                      instancePath: instancePath + '/since',
                      schemaPath: '#/properties/since/anyOf',
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
                    validate54.errors = vErrors;
                    return false;
                  } else {
                    errors = _errs14;
                    if (vErrors !== null) {
                      if (_errs14) {
                        vErrors.length = _errs14;
                      } else {
                        vErrors = null;
                      }
                    }
                  }
                  var valid0 = _errs13 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.until !== undefined) {
                    let data5 = data.until;
                    const _errs18 = errors;
                    const _errs19 = errors;
                    let valid3 = false;
                    const _errs20 = errors;
                    if (
                      !validate57(data5, {
                        instancePath: instancePath + '/until',
                        parentData: data,
                        parentDataProperty: 'until',
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate57.errors : vErrors.concat(validate57.errors);
                      errors = vErrors.length;
                    }
                    var _valid2 = _errs20 === errors;
                    valid3 = valid3 || _valid2;
                    const _errs21 = errors;
                    if (data5 !== null) {
                      const err4 = {
                        instancePath: instancePath + '/until',
                        schemaPath: '#/properties/until/anyOf/1/type',
                        keyword: 'type',
                        params: { type: 'null' },
                        message: 'must be null',
                      };
                      if (vErrors === null) {
                        vErrors = [err4];
                      } else {
                        vErrors.push(err4);
                      }
                      errors++;
                    }
                    var _valid2 = _errs21 === errors;
                    valid3 = valid3 || _valid2;
                    if (!valid3) {
                      const err5 = {
                        instancePath: instancePath + '/until',
                        schemaPath: '#/properties/until/anyOf',
                        keyword: 'anyOf',
                        params: {},
                        message: 'must match a schema in anyOf',
                      };
                      if (vErrors === null) {
                        vErrors = [err5];
                      } else {
                        vErrors.push(err5);
                      }
                      errors++;
                      validate54.errors = vErrors;
                      return false;
                    } else {
                      errors = _errs19;
                      if (vErrors !== null) {
                        if (_errs19) {
                          vErrors.length = _errs19;
                        } else {
                          vErrors = null;
                        }
                      }
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
    } else {
      validate54.errors = [
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
  validate54.errors = vErrors;
  return errors === 0;
}
validate54.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema24 = {
  additionalProperties: false,
  properties: {
    scope: { $ref: '#/$defs/MemoryScope' },
    workspace_id: { anyOf: [{ $ref: '#/$defs/WorkspaceId' }, { type: 'null' }] },
  },
  required: ['scope'],
  type: 'object',
};
const schema25 = { enum: ['global', 'project'], type: 'string' };
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
  if (typeof data !== 'string') {
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
  if (!(data === 'global' || data === 'project')) {
    validate62.errors = [
      {
        instancePath,
        schemaPath: '#/enum',
        keyword: 'enum',
        params: { allowedValues: schema25.enum },
        message: 'must be equal to one of the allowed values',
      },
    ];
    return false;
  }
  validate62.errors = vErrors;
  return errors === 0;
}
validate62.evaluated = { dynamicProps: false, dynamicItems: false };
const schema26 = {
  type: 'string',
  minLength: 30,
  maxLength: 30,
  pattern: '^wsp_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
};
const pattern4 = new RegExp('^wsp_[0-7][0-9A-HJKMNP-TV-Z]{25}$', 'u');
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
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 30) {
        validate64.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 30 },
            message: 'must NOT have more than 30 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 30) {
          validate64.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 30 },
              message: 'must NOT have fewer than 30 characters',
            },
          ];
          return false;
        } else {
          if (!pattern4.test(data)) {
            validate64.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: '^wsp_[0-7][0-9A-HJKMNP-TV-Z]{25}$' },
                message: 'must match pattern "' + '^wsp_[0-7][0-9A-HJKMNP-TV-Z]{25}$' + '"',
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
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (data.scope === undefined && (missing0 = 'scope')) {
        validate61.errors = [
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
          if (!(key0 === 'scope' || key0 === 'workspace_id')) {
            validate61.errors = [
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
          if (data.scope !== undefined) {
            const _errs2 = errors;
            if (
              !validate62(data.scope, {
                instancePath: instancePath + '/scope',
                parentData: data,
                parentDataProperty: 'scope',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate62.errors : vErrors.concat(validate62.errors);
              errors = vErrors.length;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.workspace_id !== undefined) {
              let data1 = data.workspace_id;
              const _errs3 = errors;
              const _errs4 = errors;
              let valid1 = false;
              const _errs5 = errors;
              if (
                !validate64(data1, {
                  instancePath: instancePath + '/workspace_id',
                  parentData: data,
                  parentDataProperty: 'workspace_id',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate64.errors : vErrors.concat(validate64.errors);
                errors = vErrors.length;
              }
              var _valid0 = _errs5 === errors;
              valid1 = valid1 || _valid0;
              const _errs6 = errors;
              if (data1 !== null) {
                const err0 = {
                  instancePath: instancePath + '/workspace_id',
                  schemaPath: '#/properties/workspace_id/anyOf/1/type',
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
                  instancePath: instancePath + '/workspace_id',
                  schemaPath: '#/properties/workspace_id/anyOf',
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
                validate61.errors = vErrors;
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
          }
        }
      }
    } else {
      validate61.errors = [
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
  validate61.errors = vErrors;
  return errors === 0;
}
validate61.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
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
        (data.vault === undefined && (missing0 = 'vault')) ||
        (data.query === undefined && (missing0 = 'query'))
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
          if (!(key0 === 'query' || key0 === 'vault')) {
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
          if (data.query !== undefined) {
            const _errs2 = errors;
            if (
              !validate54(data.query, {
                instancePath: instancePath + '/query',
                parentData: data,
                parentDataProperty: 'query',
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
            if (data.vault !== undefined) {
              const _errs3 = errors;
              if (
                !validate61(data.vault, {
                  instancePath: instancePath + '/vault',
                  parentData: data,
                  parentDataProperty: 'vault',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate61.errors : vErrors.concat(validate61.errors);
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
const schema27 = {
  additionalProperties: false,
  properties: {
    ids: { items: { $ref: '#/$defs/MemoryId' }, maxItems: 100, minItems: 1, type: 'array' },
    vault: { $ref: '#/$defs/MemoryVaultRequest' },
  },
  required: ['vault', 'ids'],
  type: 'object',
};
const schema28 = {
  type: 'string',
  minLength: 30,
  maxLength: 30,
  pattern: '^mem_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
};
const pattern5 = new RegExp('^mem_[0-7][0-9A-HJKMNP-TV-Z]{25}$', 'u');
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
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 30) {
        validate69.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 30 },
            message: 'must NOT have more than 30 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 30) {
          validate69.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 30 },
              message: 'must NOT have fewer than 30 characters',
            },
          ];
          return false;
        } else {
          if (!pattern5.test(data)) {
            validate69.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: '^mem_[0-7][0-9A-HJKMNP-TV-Z]{25}$' },
                message: 'must match pattern "' + '^mem_[0-7][0-9A-HJKMNP-TV-Z]{25}$' + '"',
              },
            ];
            return false;
          }
        }
      }
    } else {
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
  }
  validate69.errors = vErrors;
  return errors === 0;
}
validate69.evaluated = { dynamicProps: false, dynamicItems: false };
function validate68(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate68.evaluated;
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
        (data.vault === undefined && (missing0 = 'vault')) ||
        (data.ids === undefined && (missing0 = 'ids'))
      ) {
        validate68.errors = [
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
          if (!(key0 === 'ids' || key0 === 'vault')) {
            validate68.errors = [
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
          if (data.ids !== undefined) {
            let data0 = data.ids;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                if (data0.length > 100) {
                  validate68.errors = [
                    {
                      instancePath: instancePath + '/ids',
                      schemaPath: '#/properties/ids/maxItems',
                      keyword: 'maxItems',
                      params: { limit: 100 },
                      message: 'must NOT have more than 100 items',
                    },
                  ];
                  return false;
                } else {
                  if (data0.length < 1) {
                    validate68.errors = [
                      {
                        instancePath: instancePath + '/ids',
                        schemaPath: '#/properties/ids/minItems',
                        keyword: 'minItems',
                        params: { limit: 1 },
                        message: 'must NOT have fewer than 1 items',
                      },
                    ];
                    return false;
                  } else {
                    var valid1 = true;
                    const len0 = data0.length;
                    for (let i0 = 0; i0 < len0; i0++) {
                      const _errs4 = errors;
                      if (
                        !validate69(data0[i0], {
                          instancePath: instancePath + '/ids/' + i0,
                          parentData: data0,
                          parentDataProperty: i0,
                          rootData,
                          dynamicAnchors,
                        })
                      ) {
                        vErrors =
                          vErrors === null ? validate69.errors : vErrors.concat(validate69.errors);
                        errors = vErrors.length;
                      }
                      var valid1 = _errs4 === errors;
                      if (!valid1) {
                        break;
                      }
                    }
                  }
                }
              } else {
                validate68.errors = [
                  {
                    instancePath: instancePath + '/ids',
                    schemaPath: '#/properties/ids/type',
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
            if (data.vault !== undefined) {
              const _errs5 = errors;
              if (
                !validate61(data.vault, {
                  instancePath: instancePath + '/vault',
                  parentData: data,
                  parentDataProperty: 'vault',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate61.errors : vErrors.concat(validate61.errors);
                errors = vErrors.length;
              }
              var valid0 = _errs5 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate68.errors = [
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
  validate68.errors = vErrors;
  return errors === 0;
}
validate68.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema29 = {
  additionalProperties: false,
  properties: { workspace_id: { anyOf: [{ $ref: '#/$defs/WorkspaceId' }, { type: 'null' }] } },
  type: 'object',
};
function validate73(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate73.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      const _errs1 = errors;
      for (const key0 in data) {
        if (!(key0 === 'workspace_id')) {
          validate73.errors = [
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
        if (data.workspace_id !== undefined) {
          let data0 = data.workspace_id;
          const _errs3 = errors;
          let valid1 = false;
          const _errs4 = errors;
          if (
            !validate64(data0, {
              instancePath: instancePath + '/workspace_id',
              parentData: data,
              parentDataProperty: 'workspace_id',
              rootData,
              dynamicAnchors,
            })
          ) {
            vErrors = vErrors === null ? validate64.errors : vErrors.concat(validate64.errors);
            errors = vErrors.length;
          }
          var _valid0 = _errs4 === errors;
          valid1 = valid1 || _valid0;
          const _errs5 = errors;
          if (data0 !== null) {
            const err0 = {
              instancePath: instancePath + '/workspace_id',
              schemaPath: '#/properties/workspace_id/anyOf/1/type',
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
          var _valid0 = _errs5 === errors;
          valid1 = valid1 || _valid0;
          if (!valid1) {
            const err1 = {
              instancePath: instancePath + '/workspace_id',
              schemaPath: '#/properties/workspace_id/anyOf',
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
            validate73.errors = vErrors;
            return false;
          } else {
            errors = _errs3;
            if (vErrors !== null) {
              if (_errs3) {
                vErrors.length = _errs3;
              } else {
                vErrors = null;
              }
            }
          }
        }
      }
    } else {
      validate73.errors = [
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
  validate73.errors = vErrors;
  return errors === 0;
}
validate73.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema30 = { additionalProperties: false, type: 'object' };
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
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      for (const key0 in data) {
        validate76.errors = [
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
    } else {
      validate76.errors = [
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
  validate76.errors = vErrors;
  return errors === 0;
}
validate76.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema31 = {
  additionalProperties: false,
  properties: {
    expected_revision: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
    settings: { $ref: '#/$defs/MemorySettings' },
  },
  required: ['expected_revision', 'settings'],
  type: 'object',
};
const schema32 = {
  additionalProperties: false,
  properties: {
    automatic_maintenance: { type: 'boolean' },
    event_retention_days: { maximum: 3650, minimum: 1, type: 'integer' },
  },
  required: ['automatic_maintenance', 'event_retention_days'],
  type: 'object',
};
function validate79(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate79.evaluated;
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
        (data.automatic_maintenance === undefined && (missing0 = 'automatic_maintenance')) ||
        (data.event_retention_days === undefined && (missing0 = 'event_retention_days'))
      ) {
        validate79.errors = [
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
          if (!(key0 === 'automatic_maintenance' || key0 === 'event_retention_days')) {
            validate79.errors = [
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
          if (data.automatic_maintenance !== undefined) {
            const _errs2 = errors;
            if (typeof data.automatic_maintenance !== 'boolean') {
              validate79.errors = [
                {
                  instancePath: instancePath + '/automatic_maintenance',
                  schemaPath: '#/properties/automatic_maintenance/type',
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
            if (data.event_retention_days !== undefined) {
              let data1 = data.event_retention_days;
              const _errs4 = errors;
              if (!(typeof data1 == 'number' && !(data1 % 1) && !isNaN(data1) && isFinite(data1))) {
                validate79.errors = [
                  {
                    instancePath: instancePath + '/event_retention_days',
                    schemaPath: '#/properties/event_retention_days/type',
                    keyword: 'type',
                    params: { type: 'integer' },
                    message: 'must be integer',
                  },
                ];
                return false;
              }
              if (errors === _errs4) {
                if (typeof data1 == 'number' && isFinite(data1)) {
                  if (data1 > 3650 || isNaN(data1)) {
                    validate79.errors = [
                      {
                        instancePath: instancePath + '/event_retention_days',
                        schemaPath: '#/properties/event_retention_days/maximum',
                        keyword: 'maximum',
                        params: { comparison: '<=', limit: 3650 },
                        message: 'must be <= 3650',
                      },
                    ];
                    return false;
                  } else {
                    if (data1 < 1 || isNaN(data1)) {
                      validate79.errors = [
                        {
                          instancePath: instancePath + '/event_retention_days',
                          schemaPath: '#/properties/event_retention_days/minimum',
                          keyword: 'minimum',
                          params: { comparison: '>=', limit: 1 },
                          message: 'must be >= 1',
                        },
                      ];
                      return false;
                    }
                  }
                }
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate79.errors = [
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
  validate79.errors = vErrors;
  return errors === 0;
}
validate79.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate78(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate78.evaluated;
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
        (data.expected_revision === undefined && (missing0 = 'expected_revision')) ||
        (data.settings === undefined && (missing0 = 'settings'))
      ) {
        validate78.errors = [
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
          if (!(key0 === 'expected_revision' || key0 === 'settings')) {
            validate78.errors = [
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
          if (data.expected_revision !== undefined) {
            let data0 = data.expected_revision;
            const _errs2 = errors;
            if (!(typeof data0 == 'number' && !(data0 % 1) && !isNaN(data0) && isFinite(data0))) {
              validate78.errors = [
                {
                  instancePath: instancePath + '/expected_revision',
                  schemaPath: '#/properties/expected_revision/type',
                  keyword: 'type',
                  params: { type: 'integer' },
                  message: 'must be integer',
                },
              ];
              return false;
            }
            if (errors === _errs2) {
              if (typeof data0 == 'number' && isFinite(data0)) {
                if (data0 > 9007199254740991 || isNaN(data0)) {
                  validate78.errors = [
                    {
                      instancePath: instancePath + '/expected_revision',
                      schemaPath: '#/properties/expected_revision/maximum',
                      keyword: 'maximum',
                      params: { comparison: '<=', limit: 9007199254740991 },
                      message: 'must be <= 9007199254740991',
                    },
                  ];
                  return false;
                } else {
                  if (data0 < 0 || isNaN(data0)) {
                    validate78.errors = [
                      {
                        instancePath: instancePath + '/expected_revision',
                        schemaPath: '#/properties/expected_revision/minimum',
                        keyword: 'minimum',
                        params: { comparison: '>=', limit: 0 },
                        message: 'must be >= 0',
                      },
                    ];
                    return false;
                  }
                }
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.settings !== undefined) {
              const _errs4 = errors;
              if (
                !validate79(data.settings, {
                  instancePath: instancePath + '/settings',
                  parentData: data,
                  parentDataProperty: 'settings',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate79.errors : vErrors.concat(validate79.errors);
                errors = vErrors.length;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate78.errors = [
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
  validate78.errors = vErrors;
  return errors === 0;
}
validate78.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema33 = {
  additionalProperties: false,
  properties: { preview_id: { type: 'string' }, vault: { $ref: '#/$defs/MemoryVaultRequest' } },
  required: ['vault', 'preview_id'],
  type: 'object',
};
function validate85(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate85.evaluated;
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
        (data.vault === undefined && (missing0 = 'vault')) ||
        (data.preview_id === undefined && (missing0 = 'preview_id'))
      ) {
        validate85.errors = [
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
          if (!(key0 === 'preview_id' || key0 === 'vault')) {
            validate85.errors = [
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
          if (data.preview_id !== undefined) {
            const _errs2 = errors;
            if (typeof data.preview_id !== 'string') {
              validate85.errors = [
                {
                  instancePath: instancePath + '/preview_id',
                  schemaPath: '#/properties/preview_id/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.vault !== undefined) {
              const _errs4 = errors;
              if (
                !validate61(data.vault, {
                  instancePath: instancePath + '/vault',
                  parentData: data,
                  parentDataProperty: 'vault',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate61.errors : vErrors.concat(validate61.errors);
                errors = vErrors.length;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate85.errors = [
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
  validate85.errors = vErrors;
  return errors === 0;
}
validate85.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema34 = {
  additionalProperties: false,
  properties: {
    export_id: { type: 'string' },
    index: { minimum: 0, type: 'integer', maximum: 4294967295 },
  },
  required: ['export_id', 'index'],
  type: 'object',
};
function validate92(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate92.evaluated;
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
        (data.export_id === undefined && (missing0 = 'export_id')) ||
        (data.index === undefined && (missing0 = 'index'))
      ) {
        validate92.errors = [
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
          if (!(key0 === 'export_id' || key0 === 'index')) {
            validate92.errors = [
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
          if (data.export_id !== undefined) {
            const _errs2 = errors;
            if (typeof data.export_id !== 'string') {
              validate92.errors = [
                {
                  instancePath: instancePath + '/export_id',
                  schemaPath: '#/properties/export_id/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.index !== undefined) {
              let data1 = data.index;
              const _errs4 = errors;
              if (!(typeof data1 == 'number' && !(data1 % 1) && !isNaN(data1) && isFinite(data1))) {
                validate92.errors = [
                  {
                    instancePath: instancePath + '/index',
                    schemaPath: '#/properties/index/type',
                    keyword: 'type',
                    params: { type: 'integer' },
                    message: 'must be integer',
                  },
                ];
                return false;
              }
              if (errors === _errs4) {
                if (typeof data1 == 'number' && isFinite(data1)) {
                  if (data1 > 4294967295 || isNaN(data1)) {
                    validate92.errors = [
                      {
                        instancePath: instancePath + '/index',
                        schemaPath: '#/properties/index/maximum',
                        keyword: 'maximum',
                        params: { comparison: '<=', limit: 4294967295 },
                        message: 'must be <= 4294967295',
                      },
                    ];
                    return false;
                  } else {
                    if (data1 < 0 || isNaN(data1)) {
                      validate92.errors = [
                        {
                          instancePath: instancePath + '/index',
                          schemaPath: '#/properties/index/minimum',
                          keyword: 'minimum',
                          params: { comparison: '>=', limit: 0 },
                          message: 'must be >= 0',
                        },
                      ];
                      return false;
                    }
                  }
                }
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate92.errors = [
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
  validate92.errors = vErrors;
  return errors === 0;
}
validate92.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema35 = {
  additionalProperties: false,
  properties: {
    export_id: { type: 'string' },
    index: { minimum: 0, type: 'integer', maximum: 4294967295 },
    max_bytes: { maximum: 65536, minimum: 1, type: 'integer' },
    offset: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
  },
  required: ['export_id', 'index', 'offset', 'max_bytes'],
  type: 'object',
};
function validate94(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate94.evaluated;
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
        (data.export_id === undefined && (missing0 = 'export_id')) ||
        (data.index === undefined && (missing0 = 'index')) ||
        (data.offset === undefined && (missing0 = 'offset')) ||
        (data.max_bytes === undefined && (missing0 = 'max_bytes'))
      ) {
        validate94.errors = [
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
            key0 === 'export_id' ||
            key0 === 'index' ||
            key0 === 'max_bytes' ||
            key0 === 'offset'
          )) {
            validate94.errors = [
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
          if (data.export_id !== undefined) {
            const _errs2 = errors;
            if (typeof data.export_id !== 'string') {
              validate94.errors = [
                {
                  instancePath: instancePath + '/export_id',
                  schemaPath: '#/properties/export_id/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.index !== undefined) {
              let data1 = data.index;
              const _errs4 = errors;
              if (!(typeof data1 == 'number' && !(data1 % 1) && !isNaN(data1) && isFinite(data1))) {
                validate94.errors = [
                  {
                    instancePath: instancePath + '/index',
                    schemaPath: '#/properties/index/type',
                    keyword: 'type',
                    params: { type: 'integer' },
                    message: 'must be integer',
                  },
                ];
                return false;
              }
              if (errors === _errs4) {
                if (typeof data1 == 'number' && isFinite(data1)) {
                  if (data1 > 4294967295 || isNaN(data1)) {
                    validate94.errors = [
                      {
                        instancePath: instancePath + '/index',
                        schemaPath: '#/properties/index/maximum',
                        keyword: 'maximum',
                        params: { comparison: '<=', limit: 4294967295 },
                        message: 'must be <= 4294967295',
                      },
                    ];
                    return false;
                  } else {
                    if (data1 < 0 || isNaN(data1)) {
                      validate94.errors = [
                        {
                          instancePath: instancePath + '/index',
                          schemaPath: '#/properties/index/minimum',
                          keyword: 'minimum',
                          params: { comparison: '>=', limit: 0 },
                          message: 'must be >= 0',
                        },
                      ];
                      return false;
                    }
                  }
                }
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.max_bytes !== undefined) {
                let data2 = data.max_bytes;
                const _errs6 = errors;
                if (!(
                  typeof data2 == 'number' &&
                  !(data2 % 1) &&
                  !isNaN(data2) &&
                  isFinite(data2)
                )) {
                  validate94.errors = [
                    {
                      instancePath: instancePath + '/max_bytes',
                      schemaPath: '#/properties/max_bytes/type',
                      keyword: 'type',
                      params: { type: 'integer' },
                      message: 'must be integer',
                    },
                  ];
                  return false;
                }
                if (errors === _errs6) {
                  if (typeof data2 == 'number' && isFinite(data2)) {
                    if (data2 > 65536 || isNaN(data2)) {
                      validate94.errors = [
                        {
                          instancePath: instancePath + '/max_bytes',
                          schemaPath: '#/properties/max_bytes/maximum',
                          keyword: 'maximum',
                          params: { comparison: '<=', limit: 65536 },
                          message: 'must be <= 65536',
                        },
                      ];
                      return false;
                    } else {
                      if (data2 < 1 || isNaN(data2)) {
                        validate94.errors = [
                          {
                            instancePath: instancePath + '/max_bytes',
                            schemaPath: '#/properties/max_bytes/minimum',
                            keyword: 'minimum',
                            params: { comparison: '>=', limit: 1 },
                            message: 'must be >= 1',
                          },
                        ];
                        return false;
                      }
                    }
                  }
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.offset !== undefined) {
                  let data3 = data.offset;
                  const _errs8 = errors;
                  if (!(
                    typeof data3 == 'number' &&
                    !(data3 % 1) &&
                    !isNaN(data3) &&
                    isFinite(data3)
                  )) {
                    validate94.errors = [
                      {
                        instancePath: instancePath + '/offset',
                        schemaPath: '#/properties/offset/type',
                        keyword: 'type',
                        params: { type: 'integer' },
                        message: 'must be integer',
                      },
                    ];
                    return false;
                  }
                  if (errors === _errs8) {
                    if (typeof data3 == 'number' && isFinite(data3)) {
                      if (data3 > 9007199254740991 || isNaN(data3)) {
                        validate94.errors = [
                          {
                            instancePath: instancePath + '/offset',
                            schemaPath: '#/properties/offset/maximum',
                            keyword: 'maximum',
                            params: { comparison: '<=', limit: 9007199254740991 },
                            message: 'must be <= 9007199254740991',
                          },
                        ];
                        return false;
                      } else {
                        if (data3 < 0 || isNaN(data3)) {
                          validate94.errors = [
                            {
                              instancePath: instancePath + '/offset',
                              schemaPath: '#/properties/offset/minimum',
                              keyword: 'minimum',
                              params: { comparison: '>=', limit: 0 },
                              message: 'must be >= 0',
                            },
                          ];
                          return false;
                        }
                      }
                    }
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
      validate94.errors = [
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
  validate94.errors = vErrors;
  return errors === 0;
}
validate94.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema36 = {
  additionalProperties: false,
  properties: { export_id: { type: 'string' } },
  required: ['export_id'],
  type: 'object',
};
function validate96(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate96.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (data.export_id === undefined && (missing0 = 'export_id')) {
        validate96.errors = [
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
          if (!(key0 === 'export_id')) {
            validate96.errors = [
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
          if (data.export_id !== undefined) {
            if (typeof data.export_id !== 'string') {
              validate96.errors = [
                {
                  instancePath: instancePath + '/export_id',
                  schemaPath: '#/properties/export_id/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
          }
        }
      }
    } else {
      validate96.errors = [
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
  validate96.errors = vErrors;
  return errors === 0;
}
validate96.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
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
            if ('memory.vault.timeline' !== data.command) {
              const err2 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/0/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.timeline' },
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
            if ('memory.vault.read_many' !== data.command) {
              const err6 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/1/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.read_many' },
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
                !validate68(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate68.errors : vErrors.concat(validate68.errors);
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
            if ('memory.vaults.get' !== data.command) {
              const err10 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/2/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vaults.get' },
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
                !validate73(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate73.errors : vErrors.concat(validate73.errors);
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
            if ('memory.settings.get' !== data.command) {
              const err14 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/3/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.settings.get' },
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
                !validate76(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate76.errors : vErrors.concat(validate76.errors);
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
            if ('memory.settings.update' !== data.command) {
              const err18 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/4/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.settings.update' },
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
                !validate78(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate78.errors : vErrors.concat(validate78.errors);
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
            if ('memory.vault.validate' !== data.command) {
              const err22 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/5/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.validate' },
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
                !validate61(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate61.errors : vErrors.concat(validate61.errors);
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
            if ('memory.vault.rebuild' !== data.command) {
              const err26 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/6/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.rebuild' },
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
                !validate61(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate61.errors : vErrors.concat(validate61.errors);
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
  const _errs36 = errors;
  if (errors === _errs36) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing7;
      if (
        (data.command === undefined && (missing7 = 'command')) ||
        (data.value === undefined && (missing7 = 'value'))
      ) {
        const err28 = {
          instancePath,
          schemaPath: '#/anyOf/7/required',
          keyword: 'required',
          params: { missingProperty: missing7 },
          message: "must have required property '" + missing7 + "'",
        };
        if (vErrors === null) {
          vErrors = [err28];
        } else {
          vErrors.push(err28);
        }
        errors++;
      } else {
        const _errs38 = errors;
        for (const key7 in data) {
          if (!(key7 === 'command' || key7 === 'value')) {
            const err29 = {
              instancePath,
              schemaPath: '#/anyOf/7/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key7 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err29];
            } else {
              vErrors.push(err29);
            }
            errors++;
            break;
          }
        }
        if (_errs38 === errors) {
          if (data.command !== undefined) {
            const _errs39 = errors;
            if ('memory.vault.repair.preview' !== data.command) {
              const err30 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/7/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.repair.preview' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err30];
              } else {
                vErrors.push(err30);
              }
              errors++;
            }
            var valid8 = _errs39 === errors;
          } else {
            var valid8 = true;
          }
          if (valid8) {
            if (data.value !== undefined) {
              const _errs40 = errors;
              if (
                !validate61(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate61.errors : vErrors.concat(validate61.errors);
                errors = vErrors.length;
              }
              var valid8 = _errs40 === errors;
            } else {
              var valid8 = true;
            }
          }
        }
      }
    } else {
      const err31 = {
        instancePath,
        schemaPath: '#/anyOf/7/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err31];
      } else {
        vErrors.push(err31);
      }
      errors++;
    }
  }
  var _valid0 = _errs36 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs41 = errors;
  if (errors === _errs41) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing8;
      if (
        (data.command === undefined && (missing8 = 'command')) ||
        (data.value === undefined && (missing8 = 'value'))
      ) {
        const err32 = {
          instancePath,
          schemaPath: '#/anyOf/8/required',
          keyword: 'required',
          params: { missingProperty: missing8 },
          message: "must have required property '" + missing8 + "'",
        };
        if (vErrors === null) {
          vErrors = [err32];
        } else {
          vErrors.push(err32);
        }
        errors++;
      } else {
        const _errs43 = errors;
        for (const key8 in data) {
          if (!(key8 === 'command' || key8 === 'value')) {
            const err33 = {
              instancePath,
              schemaPath: '#/anyOf/8/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key8 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err33];
            } else {
              vErrors.push(err33);
            }
            errors++;
            break;
          }
        }
        if (_errs43 === errors) {
          if (data.command !== undefined) {
            const _errs44 = errors;
            if ('memory.vault.repair.apply' !== data.command) {
              const err34 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/8/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.repair.apply' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err34];
              } else {
                vErrors.push(err34);
              }
              errors++;
            }
            var valid9 = _errs44 === errors;
          } else {
            var valid9 = true;
          }
          if (valid9) {
            if (data.value !== undefined) {
              const _errs45 = errors;
              if (
                !validate85(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate85.errors : vErrors.concat(validate85.errors);
                errors = vErrors.length;
              }
              var valid9 = _errs45 === errors;
            } else {
              var valid9 = true;
            }
          }
        }
      }
    } else {
      const err35 = {
        instancePath,
        schemaPath: '#/anyOf/8/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err35];
      } else {
        vErrors.push(err35);
      }
      errors++;
    }
  }
  var _valid0 = _errs41 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs46 = errors;
  if (errors === _errs46) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing9;
      if (
        (data.command === undefined && (missing9 = 'command')) ||
        (data.value === undefined && (missing9 = 'value'))
      ) {
        const err36 = {
          instancePath,
          schemaPath: '#/anyOf/9/required',
          keyword: 'required',
          params: { missingProperty: missing9 },
          message: "must have required property '" + missing9 + "'",
        };
        if (vErrors === null) {
          vErrors = [err36];
        } else {
          vErrors.push(err36);
        }
        errors++;
      } else {
        const _errs48 = errors;
        for (const key9 in data) {
          if (!(key9 === 'command' || key9 === 'value')) {
            const err37 = {
              instancePath,
              schemaPath: '#/anyOf/9/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key9 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err37];
            } else {
              vErrors.push(err37);
            }
            errors++;
            break;
          }
        }
        if (_errs48 === errors) {
          if (data.command !== undefined) {
            const _errs49 = errors;
            if ('memory.vault.open_directory' !== data.command) {
              const err38 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/9/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.open_directory' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err38];
              } else {
                vErrors.push(err38);
              }
              errors++;
            }
            var valid10 = _errs49 === errors;
          } else {
            var valid10 = true;
          }
          if (valid10) {
            if (data.value !== undefined) {
              const _errs50 = errors;
              if (
                !validate61(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate61.errors : vErrors.concat(validate61.errors);
                errors = vErrors.length;
              }
              var valid10 = _errs50 === errors;
            } else {
              var valid10 = true;
            }
          }
        }
      }
    } else {
      const err39 = {
        instancePath,
        schemaPath: '#/anyOf/9/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err39];
      } else {
        vErrors.push(err39);
      }
      errors++;
    }
  }
  var _valid0 = _errs46 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs51 = errors;
  if (errors === _errs51) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing10;
      if (
        (data.command === undefined && (missing10 = 'command')) ||
        (data.value === undefined && (missing10 = 'value'))
      ) {
        const err40 = {
          instancePath,
          schemaPath: '#/anyOf/10/required',
          keyword: 'required',
          params: { missingProperty: missing10 },
          message: "must have required property '" + missing10 + "'",
        };
        if (vErrors === null) {
          vErrors = [err40];
        } else {
          vErrors.push(err40);
        }
        errors++;
      } else {
        const _errs53 = errors;
        for (const key10 in data) {
          if (!(key10 === 'command' || key10 === 'value')) {
            const err41 = {
              instancePath,
              schemaPath: '#/anyOf/10/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key10 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err41];
            } else {
              vErrors.push(err41);
            }
            errors++;
            break;
          }
        }
        if (_errs53 === errors) {
          if (data.command !== undefined) {
            const _errs54 = errors;
            if ('memory.maintenance.open_log' !== data.command) {
              const err42 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/10/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.maintenance.open_log' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err42];
              } else {
                vErrors.push(err42);
              }
              errors++;
            }
            var valid11 = _errs54 === errors;
          } else {
            var valid11 = true;
          }
          if (valid11) {
            if (data.value !== undefined) {
              const _errs55 = errors;
              if (
                !validate61(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate61.errors : vErrors.concat(validate61.errors);
                errors = vErrors.length;
              }
              var valid11 = _errs55 === errors;
            } else {
              var valid11 = true;
            }
          }
        }
      }
    } else {
      const err43 = {
        instancePath,
        schemaPath: '#/anyOf/10/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err43];
      } else {
        vErrors.push(err43);
      }
      errors++;
    }
  }
  var _valid0 = _errs51 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs56 = errors;
  if (errors === _errs56) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing11;
      if (
        (data.command === undefined && (missing11 = 'command')) ||
        (data.value === undefined && (missing11 = 'value'))
      ) {
        const err44 = {
          instancePath,
          schemaPath: '#/anyOf/11/required',
          keyword: 'required',
          params: { missingProperty: missing11 },
          message: "must have required property '" + missing11 + "'",
        };
        if (vErrors === null) {
          vErrors = [err44];
        } else {
          vErrors.push(err44);
        }
        errors++;
      } else {
        const _errs58 = errors;
        for (const key11 in data) {
          if (!(key11 === 'command' || key11 === 'value')) {
            const err45 = {
              instancePath,
              schemaPath: '#/anyOf/11/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key11 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err45];
            } else {
              vErrors.push(err45);
            }
            errors++;
            break;
          }
        }
        if (_errs58 === errors) {
          if (data.command !== undefined) {
            const _errs59 = errors;
            if ('memory.vault.export' !== data.command) {
              const err46 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/11/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.export' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err46];
              } else {
                vErrors.push(err46);
              }
              errors++;
            }
            var valid12 = _errs59 === errors;
          } else {
            var valid12 = true;
          }
          if (valid12) {
            if (data.value !== undefined) {
              const _errs60 = errors;
              if (
                !validate61(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate61.errors : vErrors.concat(validate61.errors);
                errors = vErrors.length;
              }
              var valid12 = _errs60 === errors;
            } else {
              var valid12 = true;
            }
          }
        }
      }
    } else {
      const err47 = {
        instancePath,
        schemaPath: '#/anyOf/11/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err47];
      } else {
        vErrors.push(err47);
      }
      errors++;
    }
  }
  var _valid0 = _errs56 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs61 = errors;
  if (errors === _errs61) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing12;
      if (
        (data.command === undefined && (missing12 = 'command')) ||
        (data.value === undefined && (missing12 = 'value'))
      ) {
        const err48 = {
          instancePath,
          schemaPath: '#/anyOf/12/required',
          keyword: 'required',
          params: { missingProperty: missing12 },
          message: "must have required property '" + missing12 + "'",
        };
        if (vErrors === null) {
          vErrors = [err48];
        } else {
          vErrors.push(err48);
        }
        errors++;
      } else {
        const _errs63 = errors;
        for (const key12 in data) {
          if (!(key12 === 'command' || key12 === 'value')) {
            const err49 = {
              instancePath,
              schemaPath: '#/anyOf/12/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key12 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err49];
            } else {
              vErrors.push(err49);
            }
            errors++;
            break;
          }
        }
        if (_errs63 === errors) {
          if (data.command !== undefined) {
            const _errs64 = errors;
            if ('memory.export.prepare' !== data.command) {
              const err50 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/12/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.export.prepare' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err50];
              } else {
                vErrors.push(err50);
              }
              errors++;
            }
            var valid13 = _errs64 === errors;
          } else {
            var valid13 = true;
          }
          if (valid13) {
            if (data.value !== undefined) {
              const _errs65 = errors;
              if (
                !validate61(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate61.errors : vErrors.concat(validate61.errors);
                errors = vErrors.length;
              }
              var valid13 = _errs65 === errors;
            } else {
              var valid13 = true;
            }
          }
        }
      }
    } else {
      const err51 = {
        instancePath,
        schemaPath: '#/anyOf/12/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err51];
      } else {
        vErrors.push(err51);
      }
      errors++;
    }
  }
  var _valid0 = _errs61 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs66 = errors;
  if (errors === _errs66) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing13;
      if (
        (data.command === undefined && (missing13 = 'command')) ||
        (data.value === undefined && (missing13 = 'value'))
      ) {
        const err52 = {
          instancePath,
          schemaPath: '#/anyOf/13/required',
          keyword: 'required',
          params: { missingProperty: missing13 },
          message: "must have required property '" + missing13 + "'",
        };
        if (vErrors === null) {
          vErrors = [err52];
        } else {
          vErrors.push(err52);
        }
        errors++;
      } else {
        const _errs68 = errors;
        for (const key13 in data) {
          if (!(key13 === 'command' || key13 === 'value')) {
            const err53 = {
              instancePath,
              schemaPath: '#/anyOf/13/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key13 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err53];
            } else {
              vErrors.push(err53);
            }
            errors++;
            break;
          }
        }
        if (_errs68 === errors) {
          if (data.command !== undefined) {
            const _errs69 = errors;
            if ('memory.export.entry' !== data.command) {
              const err54 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/13/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.export.entry' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err54];
              } else {
                vErrors.push(err54);
              }
              errors++;
            }
            var valid14 = _errs69 === errors;
          } else {
            var valid14 = true;
          }
          if (valid14) {
            if (data.value !== undefined) {
              const _errs70 = errors;
              if (
                !validate92(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate92.errors : vErrors.concat(validate92.errors);
                errors = vErrors.length;
              }
              var valid14 = _errs70 === errors;
            } else {
              var valid14 = true;
            }
          }
        }
      }
    } else {
      const err55 = {
        instancePath,
        schemaPath: '#/anyOf/13/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err55];
      } else {
        vErrors.push(err55);
      }
      errors++;
    }
  }
  var _valid0 = _errs66 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs71 = errors;
  if (errors === _errs71) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing14;
      if (
        (data.command === undefined && (missing14 = 'command')) ||
        (data.value === undefined && (missing14 = 'value'))
      ) {
        const err56 = {
          instancePath,
          schemaPath: '#/anyOf/14/required',
          keyword: 'required',
          params: { missingProperty: missing14 },
          message: "must have required property '" + missing14 + "'",
        };
        if (vErrors === null) {
          vErrors = [err56];
        } else {
          vErrors.push(err56);
        }
        errors++;
      } else {
        const _errs73 = errors;
        for (const key14 in data) {
          if (!(key14 === 'command' || key14 === 'value')) {
            const err57 = {
              instancePath,
              schemaPath: '#/anyOf/14/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key14 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err57];
            } else {
              vErrors.push(err57);
            }
            errors++;
            break;
          }
        }
        if (_errs73 === errors) {
          if (data.command !== undefined) {
            const _errs74 = errors;
            if ('memory.export.chunk' !== data.command) {
              const err58 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/14/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.export.chunk' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err58];
              } else {
                vErrors.push(err58);
              }
              errors++;
            }
            var valid15 = _errs74 === errors;
          } else {
            var valid15 = true;
          }
          if (valid15) {
            if (data.value !== undefined) {
              const _errs75 = errors;
              if (
                !validate94(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate94.errors : vErrors.concat(validate94.errors);
                errors = vErrors.length;
              }
              var valid15 = _errs75 === errors;
            } else {
              var valid15 = true;
            }
          }
        }
      }
    } else {
      const err59 = {
        instancePath,
        schemaPath: '#/anyOf/14/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err59];
      } else {
        vErrors.push(err59);
      }
      errors++;
    }
  }
  var _valid0 = _errs71 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs76 = errors;
  if (errors === _errs76) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing15;
      if (
        (data.command === undefined && (missing15 = 'command')) ||
        (data.value === undefined && (missing15 = 'value'))
      ) {
        const err60 = {
          instancePath,
          schemaPath: '#/anyOf/15/required',
          keyword: 'required',
          params: { missingProperty: missing15 },
          message: "must have required property '" + missing15 + "'",
        };
        if (vErrors === null) {
          vErrors = [err60];
        } else {
          vErrors.push(err60);
        }
        errors++;
      } else {
        const _errs78 = errors;
        for (const key15 in data) {
          if (!(key15 === 'command' || key15 === 'value')) {
            const err61 = {
              instancePath,
              schemaPath: '#/anyOf/15/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key15 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err61];
            } else {
              vErrors.push(err61);
            }
            errors++;
            break;
          }
        }
        if (_errs78 === errors) {
          if (data.command !== undefined) {
            const _errs79 = errors;
            if ('memory.export.close' !== data.command) {
              const err62 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/15/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.export.close' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err62];
              } else {
                vErrors.push(err62);
              }
              errors++;
            }
            var valid16 = _errs79 === errors;
          } else {
            var valid16 = true;
          }
          if (valid16) {
            if (data.value !== undefined) {
              const _errs80 = errors;
              if (
                !validate96(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate96.errors : vErrors.concat(validate96.errors);
                errors = vErrors.length;
              }
              var valid16 = _errs80 === errors;
            } else {
              var valid16 = true;
            }
          }
        }
      }
    } else {
      const err63 = {
        instancePath,
        schemaPath: '#/anyOf/15/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err63];
      } else {
        vErrors.push(err63);
      }
      errors++;
    }
  }
  var _valid0 = _errs76 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  if (!valid0) {
    const err64 = {
      instancePath,
      schemaPath: '#/anyOf',
      keyword: 'anyOf',
      params: {},
      message: 'must match a schema in anyOf',
    };
    if (vErrors === null) {
      vErrors = [err64];
    } else {
      vErrors.push(err64);
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
export const validateMemoryResponse = validate98;
const schema37 = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  anyOf: [
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.timeline' },
        value: { $ref: '#/$defs/MemoryTimeline' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.read_many' },
        value: { $ref: '#/$defs/MemoryBatchRead' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vaults.get' },
        value: { $ref: '#/$defs/MemoryDesktopSnapshot' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.settings.get' },
        value: { $ref: '#/$defs/MemorySettingsSnapshot' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.settings.update' },
        value: { $ref: '#/$defs/MemorySettingsSnapshot' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.validate' },
        value: { $ref: '#/$defs/MemoryValidation' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.rebuild' },
        value: { $ref: '#/$defs/MemoryVaultStatus' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.repair.preview' },
        value: { $ref: '#/$defs/MemoryRepairPreview' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.repair.apply' },
        value: { $ref: '#/$defs/MemoryRepairResult' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.open_directory' },
        value: { $ref: '#/$defs/MemoryDesktopActionResult' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.maintenance.open_log' },
        value: { $ref: '#/$defs/MemoryDesktopActionResult' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.vault.export' },
        value: { $ref: '#/$defs/MemoryDesktopActionResult' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.export.prepare' },
        value: { $ref: '#/$defs/MemoryExportHandle' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.export.entry' },
        value: { $ref: '#/$defs/MemoryExportEntry' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.export.chunk' },
        value: { $ref: '#/$defs/MemoryExportChunk' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'memory.export.close' },
        value: { $ref: '#/$defs/MemoryDesktopActionResult' },
      },
    },
  ],
  $defs: {
    MemoryTimelineRequest: {
      additionalProperties: false,
      properties: {
        query: { $ref: '#/$defs/MemoryTimelineQuery' },
        vault: { $ref: '#/$defs/MemoryVaultRequest' },
      },
      required: ['vault', 'query'],
      type: 'object',
    },
    MemoryTimelineQuery: {
      additionalProperties: false,
      properties: {
        author_session_id: { anyOf: [{ $ref: '#/$defs/SessionId' }, { type: 'null' }] },
        before: {
          description: 'The record hash, not a row number tied to one SQLite rebuild.',
          type: ['string', 'null'],
        },
        category: {
          description: 'Current category of the note; deleted/unindexed notes have no category.',
          type: ['string', 'null'],
        },
        limit: { maximum: 100, minimum: 1, type: ['integer', 'null'] },
        since: { anyOf: [{ $ref: '#/$defs/UtcTimestamp' }, { type: 'null' }] },
        until: { anyOf: [{ $ref: '#/$defs/UtcTimestamp' }, { type: 'null' }] },
      },
      type: 'object',
    },
    SessionId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    UtcTimestamp: { type: 'string' },
    MemoryVaultRequest: {
      additionalProperties: false,
      properties: {
        scope: { $ref: '#/$defs/MemoryScope' },
        workspace_id: { anyOf: [{ $ref: '#/$defs/WorkspaceId' }, { type: 'null' }] },
      },
      required: ['scope'],
      type: 'object',
    },
    MemoryScope: { enum: ['global', 'project'], type: 'string' },
    WorkspaceId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^wsp_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    MemoryTimeline: {
      additionalProperties: false,
      properties: {
        before: { type: ['string', 'null'] },
        entries: { items: { $ref: '#/$defs/MemoryTimelineEntry' }, type: 'array' },
        incomplete: { type: 'boolean' },
      },
      required: ['entries', 'incomplete'],
      type: 'object',
    },
    MemoryTimelineEntry: {
      additionalProperties: false,
      properties: {
        category: { type: ['string', 'null'] },
        cursor: { type: 'string' },
        event: { $ref: '#/$defs/MemoryHistory' },
        session_name_at_write: { type: ['string', 'null'] },
      },
      required: ['cursor', 'event'],
      type: 'object',
    },
    MemoryHistory: {
      additionalProperties: false,
      properties: {
        action: { type: 'string' },
        actor: { type: 'string' },
        after_hash: { type: ['string', 'null'] },
        at: { $ref: '#/$defs/UtcTimestamp' },
        before_hash: { type: ['string', 'null'] },
        id: { anyOf: [{ $ref: '#/$defs/MemoryId' }, { type: 'null' }] },
        path: { type: 'string' },
        session_id: { anyOf: [{ $ref: '#/$defs/SessionId' }, { type: 'null' }] },
        title: { type: 'string' },
        turn_id: { type: ['string', 'null'] },
      },
      required: ['path', 'action', 'actor', 'title', 'at'],
      type: 'object',
    },
    MemoryId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^mem_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    MemoryBatchReadRequest: {
      additionalProperties: false,
      properties: {
        ids: { items: { $ref: '#/$defs/MemoryId' }, maxItems: 100, minItems: 1, type: 'array' },
        vault: { $ref: '#/$defs/MemoryVaultRequest' },
      },
      required: ['vault', 'ids'],
      type: 'object',
    },
    MemoryBatchRead: {
      additionalProperties: false,
      properties: {
        missing_ids: { items: { $ref: '#/$defs/MemoryId' }, type: 'array' },
        notes: { items: { $ref: '#/$defs/MemoryRead' }, type: 'array' },
      },
      required: ['notes', 'missing_ids'],
      type: 'object',
    },
    MemoryRead: {
      additionalProperties: false,
      properties: {
        hash: { type: 'string' },
        history: { items: { $ref: '#/$defs/MemoryHistory' }, type: 'array' },
        note: { $ref: '#/$defs/MemoryDocument' },
        path: { type: 'string' },
        provenance: { anyOf: [{ $ref: '#/$defs/MemoryProvenance' }, { type: 'null' }] },
      },
      required: ['path', 'note', 'hash', 'history'],
      type: 'object',
    },
    MemoryDocument: {
      additionalProperties: false,
      properties: { body: { type: 'string' }, metadata: { $ref: '#/$defs/MemoryMetadata' } },
      required: ['metadata', 'body'],
      type: 'object',
    },
    MemoryMetadata: {
      additionalProperties: false,
      properties: {
        category: { type: 'string' },
        confidence: { type: 'number' },
        conflicts_with_ids: { items: { $ref: '#/$defs/MemoryId' }, type: 'array' },
        created_at: { $ref: '#/$defs/UtcTimestamp' },
        id: { $ref: '#/$defs/MemoryId' },
        importance: { maximum: 255, minimum: 0, type: 'integer' },
        last_confirmed_at: { anyOf: [{ $ref: '#/$defs/UtcTimestamp' }, { type: 'null' }] },
        recall_weight: { type: 'number' },
        related_ids: { items: { $ref: '#/$defs/MemoryId' }, type: 'array' },
        schema_version: { minimum: 0, type: 'integer', maximum: 4294967295 },
        scope: { $ref: '#/$defs/MemoryScope' },
        source_events: { items: { type: 'string' }, type: 'array' },
        source_sessions: { items: { $ref: '#/$defs/SessionId' }, type: 'array' },
        status: { $ref: '#/$defs/MemoryStatus' },
        supersedes_ids: { items: { $ref: '#/$defs/MemoryId' }, type: 'array' },
        tags: { items: { type: 'string' }, type: 'array' },
        updated_at: { $ref: '#/$defs/UtcTimestamp' },
      },
      required: [
        'schema_version',
        'id',
        'scope',
        'category',
        'status',
        'confidence',
        'importance',
        'recall_weight',
        'created_at',
        'updated_at',
        'source_sessions',
        'source_events',
        'tags',
        'related_ids',
        'supersedes_ids',
        'conflicts_with_ids',
      ],
      type: 'object',
    },
    MemoryStatus: {
      enum: ['active', 'superseded', 'forgotten', 'conflicted', 'archived'],
      type: 'string',
    },
    MemoryProvenance: {
      additionalProperties: false,
      properties: {
        creator: {
          anyOf: [{ $ref: '#/$defs/MemoryAuthorStamp' }, { type: 'null' }],
          description:
            'None means there is no reliable creation record. Source sessions alone are not proof.',
        },
        modifiers: {
          description: 'New authorship stamps plus recent known events, most recent last.',
          items: { $ref: '#/$defs/MemoryAuthorStamp' },
          type: 'array',
        },
      },
      required: ['modifiers'],
      type: 'object',
    },
    MemoryAuthorStamp: {
      additionalProperties: false,
      properties: {
        actor: { type: 'string' },
        at: { $ref: '#/$defs/UtcTimestamp' },
        session_id: { anyOf: [{ $ref: '#/$defs/SessionId' }, { type: 'null' }] },
        session_name_at_write: { type: ['string', 'null'] },
      },
      required: ['actor', 'at'],
      type: 'object',
    },
    MemoryDesktopRequest: {
      additionalProperties: false,
      properties: { workspace_id: { anyOf: [{ $ref: '#/$defs/WorkspaceId' }, { type: 'null' }] } },
      type: 'object',
    },
    MemoryDesktopSnapshot: {
      additionalProperties: false,
      properties: {
        recent_runs: { items: { $ref: '#/$defs/MemoryMaintenanceRun' }, type: 'array' },
        vaults: { items: { $ref: '#/$defs/MemoryVaultStatus' }, type: 'array' },
      },
      required: ['vaults', 'recent_runs'],
      type: 'object',
    },
    MemoryMaintenanceRun: {
      additionalProperties: false,
      properties: {
        finished_at: { anyOf: [{ $ref: '#/$defs/UtcTimestamp' }, { type: 'null' }] },
        input_tokens: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        model_profile_id: { anyOf: [{ $ref: '#/$defs/ModelProfileId' }, { type: 'null' }] },
        output_tokens: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        reason: { type: ['string', 'null'] },
        run_id: { type: 'string' },
        runtime_instance_id: { $ref: '#/$defs/RuntimeInstanceId' },
        session_id: { $ref: '#/$defs/SessionId' },
        started_at: { $ref: '#/$defs/UtcTimestamp' },
        state: { $ref: '#/$defs/MemoryMaintenanceState' },
        turn_id: { type: ['string', 'null'] },
        usage_complete: {
          default: false,
          description: 'Counts observed so far; false means final Provider usage is unconfirmed.',
          type: 'boolean',
        },
      },
      required: [
        'run_id',
        'runtime_instance_id',
        'session_id',
        'state',
        'input_tokens',
        'output_tokens',
        'started_at',
      ],
      type: 'object',
    },
    ModelProfileId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^mdl_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    RuntimeInstanceId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^run_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    MemoryMaintenanceState: {
      enum: ['queued', 'running', 'completed', 'skipped', 'failed', 'interrupted'],
      type: 'string',
    },
    MemoryVaultStatus: {
      additionalProperties: false,
      properties: {
        directory: { type: 'string' },
        exists: { type: 'boolean' },
        invalid: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        maintenance_log: {
          default: null,
          description:
            "The plain text log of this Vault's finished maintenance runs, once one was written.",
          type: ['string', 'null'],
        },
        rebuilding: { type: 'boolean' },
        recent_changes: { items: { $ref: '#/$defs/MemoryHistory' }, type: 'array' },
        scope: { $ref: '#/$defs/MemoryScope' },
        valid: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        watcher_error: { type: ['string', 'null'] },
      },
      required: [
        'scope',
        'directory',
        'exists',
        'valid',
        'invalid',
        'rebuilding',
        'recent_changes',
      ],
      type: 'object',
    },
    EmptyToolInput: { additionalProperties: false, type: 'object' },
    MemorySettingsSnapshot: {
      additionalProperties: false,
      properties: {
        issue: { type: ['string', 'null'] },
        revision: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
        settings: { $ref: '#/$defs/MemorySettings' },
      },
      required: ['settings'],
      type: 'object',
    },
    MemorySettings: {
      additionalProperties: false,
      properties: {
        automatic_maintenance: { type: 'boolean' },
        event_retention_days: { maximum: 3650, minimum: 1, type: 'integer' },
      },
      required: ['automatic_maintenance', 'event_retention_days'],
      type: 'object',
    },
    MemorySettingsUpdate: {
      additionalProperties: false,
      properties: {
        expected_revision: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        settings: { $ref: '#/$defs/MemorySettings' },
      },
      required: ['expected_revision', 'settings'],
      type: 'object',
    },
    MemoryValidation: {
      additionalProperties: false,
      properties: {
        invalid: { items: { $ref: '#/$defs/MemoryInvalidFile' }, type: 'array' },
        rebuilding: { type: 'boolean' },
        valid: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
      },
      required: ['valid', 'invalid', 'rebuilding'],
      type: 'object',
    },
    MemoryInvalidFile: {
      additionalProperties: false,
      properties: { path: { type: 'string' }, reason: { type: 'string' } },
      required: ['path', 'reason'],
      type: 'object',
    },
    MemoryRepairPreview: {
      additionalProperties: false,
      properties: {
        changes: { items: { $ref: '#/$defs/MemoryRepairChange' }, type: 'array' },
        manual: { items: { $ref: '#/$defs/MemoryInvalidFile' }, type: 'array' },
        preview_token: { type: 'string' },
        rebuild_index: { type: 'boolean' },
      },
      required: ['preview_token', 'changes', 'manual', 'rebuild_index'],
      type: 'object',
    },
    MemoryRepairChange: {
      additionalProperties: false,
      properties: {
        changes: { items: { type: 'string' }, type: 'array' },
        expected_hash: { type: ['string', 'null'] },
        path: { type: 'string' },
        proposed_hash: { type: 'string' },
      },
      required: ['path', 'proposed_hash', 'changes'],
      type: 'object',
    },
    MemoryDesktopRepairApply: {
      additionalProperties: false,
      properties: { preview_id: { type: 'string' }, vault: { $ref: '#/$defs/MemoryVaultRequest' } },
      required: ['vault', 'preview_id'],
      type: 'object',
    },
    MemoryRepairResult: {
      additionalProperties: false,
      properties: {
        applied: { type: 'boolean' },
        preview: { $ref: '#/$defs/MemoryRepairPreview' },
      },
      required: ['preview', 'applied'],
      type: 'object',
    },
    MemoryDesktopActionResult: {
      additionalProperties: false,
      properties: { completed: { type: 'boolean' }, message: { type: 'string' } },
      required: ['completed', 'message'],
      type: 'object',
    },
    MemoryExportHandle: {
      additionalProperties: false,
      properties: {
        entries: { minimum: 0, type: 'integer', maximum: 4294967295 },
        export_id: { type: 'string' },
        total_bytes: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
      },
      required: ['export_id', 'entries', 'total_bytes'],
      type: 'object',
    },
    MemoryExportEntryRequest: {
      additionalProperties: false,
      properties: {
        export_id: { type: 'string' },
        index: { minimum: 0, type: 'integer', maximum: 4294967295 },
      },
      required: ['export_id', 'index'],
      type: 'object',
    },
    MemoryExportEntry: {
      additionalProperties: false,
      properties: {
        directory: { type: 'boolean' },
        length: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        path: { type: 'string' },
        sha256: { type: 'string' },
      },
      required: ['path', 'directory', 'length', 'sha256'],
      type: 'object',
    },
    MemoryExportChunkRequest: {
      additionalProperties: false,
      properties: {
        export_id: { type: 'string' },
        index: { minimum: 0, type: 'integer', maximum: 4294967295 },
        max_bytes: { maximum: 65536, minimum: 1, type: 'integer' },
        offset: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
      },
      required: ['export_id', 'index', 'offset', 'max_bytes'],
      type: 'object',
    },
    MemoryExportChunk: {
      additionalProperties: false,
      properties: {
        bytes: { items: { maximum: 255, minimum: 0, type: 'integer' }, type: 'array' },
        eof: { type: 'boolean' },
        next_offset: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        offset: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
      },
      required: ['offset', 'next_offset', 'bytes', 'eof'],
      type: 'object',
    },
    MemoryExportCloseRequest: {
      additionalProperties: false,
      properties: { export_id: { type: 'string' } },
      required: ['export_id'],
      type: 'object',
    },
  },
};
const schema38 = {
  additionalProperties: false,
  properties: {
    before: { type: ['string', 'null'] },
    entries: { items: { $ref: '#/$defs/MemoryTimelineEntry' }, type: 'array' },
    incomplete: { type: 'boolean' },
  },
  required: ['entries', 'incomplete'],
  type: 'object',
};
const schema39 = {
  additionalProperties: false,
  properties: {
    category: { type: ['string', 'null'] },
    cursor: { type: 'string' },
    event: { $ref: '#/$defs/MemoryHistory' },
    session_name_at_write: { type: ['string', 'null'] },
  },
  required: ['cursor', 'event'],
  type: 'object',
};
const schema40 = {
  additionalProperties: false,
  properties: {
    action: { type: 'string' },
    actor: { type: 'string' },
    after_hash: { type: ['string', 'null'] },
    at: { $ref: '#/$defs/UtcTimestamp' },
    before_hash: { type: ['string', 'null'] },
    id: { anyOf: [{ $ref: '#/$defs/MemoryId' }, { type: 'null' }] },
    path: { type: 'string' },
    session_id: { anyOf: [{ $ref: '#/$defs/SessionId' }, { type: 'null' }] },
    title: { type: 'string' },
    turn_id: { type: ['string', 'null'] },
  },
  required: ['path', 'action', 'actor', 'title', 'at'],
  type: 'object',
};
const func7 = Object.prototype.hasOwnProperty;
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
  if (typeof data !== 'string') {
    validate102.errors = [
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
  validate102.errors = vErrors;
  return errors === 0;
}
validate102.evaluated = { dynamicProps: false, dynamicItems: false };
function validate104(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate104.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 30) {
        validate104.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 30 },
            message: 'must NOT have more than 30 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 30) {
          validate104.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 30 },
              message: 'must NOT have fewer than 30 characters',
            },
          ];
          return false;
        } else {
          if (!pattern5.test(data)) {
            validate104.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: '^mem_[0-7][0-9A-HJKMNP-TV-Z]{25}$' },
                message: 'must match pattern "' + '^mem_[0-7][0-9A-HJKMNP-TV-Z]{25}$' + '"',
              },
            ];
            return false;
          }
        }
      }
    } else {
      validate104.errors = [
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
  validate104.errors = vErrors;
  return errors === 0;
}
validate104.evaluated = { dynamicProps: false, dynamicItems: false };
function validate106(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate106.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 30) {
        validate106.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 30 },
            message: 'must NOT have more than 30 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 30) {
          validate106.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 30 },
              message: 'must NOT have fewer than 30 characters',
            },
          ];
          return false;
        } else {
          if (!pattern3.test(data)) {
            validate106.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: '^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$' },
                message: 'must match pattern "' + '^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$' + '"',
              },
            ];
            return false;
          }
        }
      }
    } else {
      validate106.errors = [
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
  validate106.errors = vErrors;
  return errors === 0;
}
validate106.evaluated = { dynamicProps: false, dynamicItems: false };
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
        (data.path === undefined && (missing0 = 'path')) ||
        (data.action === undefined && (missing0 = 'action')) ||
        (data.actor === undefined && (missing0 = 'actor')) ||
        (data.title === undefined && (missing0 = 'title')) ||
        (data.at === undefined && (missing0 = 'at'))
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
          if (!func7.call(schema40.properties, key0)) {
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
          if (data.action !== undefined) {
            const _errs2 = errors;
            if (typeof data.action !== 'string') {
              validate101.errors = [
                {
                  instancePath: instancePath + '/action',
                  schemaPath: '#/properties/action/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.actor !== undefined) {
              const _errs4 = errors;
              if (typeof data.actor !== 'string') {
                validate101.errors = [
                  {
                    instancePath: instancePath + '/actor',
                    schemaPath: '#/properties/actor/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.after_hash !== undefined) {
                let data2 = data.after_hash;
                const _errs6 = errors;
                if (typeof data2 !== 'string' && data2 !== null) {
                  validate101.errors = [
                    {
                      instancePath: instancePath + '/after_hash',
                      schemaPath: '#/properties/after_hash/type',
                      keyword: 'type',
                      params: { type: schema40.properties.after_hash.type },
                      message: 'must be string,null',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.at !== undefined) {
                  const _errs8 = errors;
                  if (
                    !validate102(data.at, {
                      instancePath: instancePath + '/at',
                      parentData: data,
                      parentDataProperty: 'at',
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate102.errors : vErrors.concat(validate102.errors);
                    errors = vErrors.length;
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.before_hash !== undefined) {
                    let data4 = data.before_hash;
                    const _errs9 = errors;
                    if (typeof data4 !== 'string' && data4 !== null) {
                      validate101.errors = [
                        {
                          instancePath: instancePath + '/before_hash',
                          schemaPath: '#/properties/before_hash/type',
                          keyword: 'type',
                          params: { type: schema40.properties.before_hash.type },
                          message: 'must be string,null',
                        },
                      ];
                      return false;
                    }
                    var valid0 = _errs9 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.id !== undefined) {
                      let data5 = data.id;
                      const _errs11 = errors;
                      const _errs12 = errors;
                      let valid1 = false;
                      const _errs13 = errors;
                      if (
                        !validate104(data5, {
                          instancePath: instancePath + '/id',
                          parentData: data,
                          parentDataProperty: 'id',
                          rootData,
                          dynamicAnchors,
                        })
                      ) {
                        vErrors =
                          vErrors === null
                            ? validate104.errors
                            : vErrors.concat(validate104.errors);
                        errors = vErrors.length;
                      }
                      var _valid0 = _errs13 === errors;
                      valid1 = valid1 || _valid0;
                      const _errs14 = errors;
                      if (data5 !== null) {
                        const err0 = {
                          instancePath: instancePath + '/id',
                          schemaPath: '#/properties/id/anyOf/1/type',
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
                      var _valid0 = _errs14 === errors;
                      valid1 = valid1 || _valid0;
                      if (!valid1) {
                        const err1 = {
                          instancePath: instancePath + '/id',
                          schemaPath: '#/properties/id/anyOf',
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
                    if (valid0) {
                      if (data.path !== undefined) {
                        const _errs16 = errors;
                        if (typeof data.path !== 'string') {
                          validate101.errors = [
                            {
                              instancePath: instancePath + '/path',
                              schemaPath: '#/properties/path/type',
                              keyword: 'type',
                              params: { type: 'string' },
                              message: 'must be string',
                            },
                          ];
                          return false;
                        }
                        var valid0 = _errs16 === errors;
                      } else {
                        var valid0 = true;
                      }
                      if (valid0) {
                        if (data.session_id !== undefined) {
                          let data7 = data.session_id;
                          const _errs18 = errors;
                          const _errs19 = errors;
                          let valid2 = false;
                          const _errs20 = errors;
                          if (
                            !validate106(data7, {
                              instancePath: instancePath + '/session_id',
                              parentData: data,
                              parentDataProperty: 'session_id',
                              rootData,
                              dynamicAnchors,
                            })
                          ) {
                            vErrors =
                              vErrors === null
                                ? validate106.errors
                                : vErrors.concat(validate106.errors);
                            errors = vErrors.length;
                          }
                          var _valid1 = _errs20 === errors;
                          valid2 = valid2 || _valid1;
                          const _errs21 = errors;
                          if (data7 !== null) {
                            const err2 = {
                              instancePath: instancePath + '/session_id',
                              schemaPath: '#/properties/session_id/anyOf/1/type',
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
                          var _valid1 = _errs21 === errors;
                          valid2 = valid2 || _valid1;
                          if (!valid2) {
                            const err3 = {
                              instancePath: instancePath + '/session_id',
                              schemaPath: '#/properties/session_id/anyOf',
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
                            validate101.errors = vErrors;
                            return false;
                          } else {
                            errors = _errs19;
                            if (vErrors !== null) {
                              if (_errs19) {
                                vErrors.length = _errs19;
                              } else {
                                vErrors = null;
                              }
                            }
                          }
                          var valid0 = _errs18 === errors;
                        } else {
                          var valid0 = true;
                        }
                        if (valid0) {
                          if (data.title !== undefined) {
                            const _errs23 = errors;
                            if (typeof data.title !== 'string') {
                              validate101.errors = [
                                {
                                  instancePath: instancePath + '/title',
                                  schemaPath: '#/properties/title/type',
                                  keyword: 'type',
                                  params: { type: 'string' },
                                  message: 'must be string',
                                },
                              ];
                              return false;
                            }
                            var valid0 = _errs23 === errors;
                          } else {
                            var valid0 = true;
                          }
                          if (valid0) {
                            if (data.turn_id !== undefined) {
                              let data9 = data.turn_id;
                              const _errs25 = errors;
                              if (typeof data9 !== 'string' && data9 !== null) {
                                validate101.errors = [
                                  {
                                    instancePath: instancePath + '/turn_id',
                                    schemaPath: '#/properties/turn_id/type',
                                    keyword: 'type',
                                    params: { type: schema40.properties.turn_id.type },
                                    message: 'must be string,null',
                                  },
                                ];
                                return false;
                              }
                              var valid0 = _errs25 === errors;
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
function validate100(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate100.evaluated;
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
        (data.cursor === undefined && (missing0 = 'cursor')) ||
        (data.event === undefined && (missing0 = 'event'))
      ) {
        validate100.errors = [
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
            key0 === 'category' ||
            key0 === 'cursor' ||
            key0 === 'event' ||
            key0 === 'session_name_at_write'
          )) {
            validate100.errors = [
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
          if (data.category !== undefined) {
            let data0 = data.category;
            const _errs2 = errors;
            if (typeof data0 !== 'string' && data0 !== null) {
              validate100.errors = [
                {
                  instancePath: instancePath + '/category',
                  schemaPath: '#/properties/category/type',
                  keyword: 'type',
                  params: { type: schema39.properties.category.type },
                  message: 'must be string,null',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.cursor !== undefined) {
              const _errs4 = errors;
              if (typeof data.cursor !== 'string') {
                validate100.errors = [
                  {
                    instancePath: instancePath + '/cursor',
                    schemaPath: '#/properties/cursor/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.event !== undefined) {
                const _errs6 = errors;
                if (
                  !validate101(data.event, {
                    instancePath: instancePath + '/event',
                    parentData: data,
                    parentDataProperty: 'event',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate101.errors : vErrors.concat(validate101.errors);
                  errors = vErrors.length;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.session_name_at_write !== undefined) {
                  let data3 = data.session_name_at_write;
                  const _errs7 = errors;
                  if (typeof data3 !== 'string' && data3 !== null) {
                    validate100.errors = [
                      {
                        instancePath: instancePath + '/session_name_at_write',
                        schemaPath: '#/properties/session_name_at_write/type',
                        keyword: 'type',
                        params: { type: schema39.properties.session_name_at_write.type },
                        message: 'must be string,null',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs7 === errors;
                } else {
                  var valid0 = true;
                }
              }
            }
          }
        }
      }
    } else {
      validate100.errors = [
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
  validate100.errors = vErrors;
  return errors === 0;
}
validate100.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
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
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.entries === undefined && (missing0 = 'entries')) ||
        (data.incomplete === undefined && (missing0 = 'incomplete'))
      ) {
        validate99.errors = [
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
          if (!(key0 === 'before' || key0 === 'entries' || key0 === 'incomplete')) {
            validate99.errors = [
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
          if (data.before !== undefined) {
            let data0 = data.before;
            const _errs2 = errors;
            if (typeof data0 !== 'string' && data0 !== null) {
              validate99.errors = [
                {
                  instancePath: instancePath + '/before',
                  schemaPath: '#/properties/before/type',
                  keyword: 'type',
                  params: { type: schema38.properties.before.type },
                  message: 'must be string,null',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.entries !== undefined) {
              let data1 = data.entries;
              const _errs4 = errors;
              if (errors === _errs4) {
                if (Array.isArray(data1)) {
                  var valid1 = true;
                  const len0 = data1.length;
                  for (let i0 = 0; i0 < len0; i0++) {
                    const _errs6 = errors;
                    if (
                      !validate100(data1[i0], {
                        instancePath: instancePath + '/entries/' + i0,
                        parentData: data1,
                        parentDataProperty: i0,
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate100.errors : vErrors.concat(validate100.errors);
                      errors = vErrors.length;
                    }
                    var valid1 = _errs6 === errors;
                    if (!valid1) {
                      break;
                    }
                  }
                } else {
                  validate99.errors = [
                    {
                      instancePath: instancePath + '/entries',
                      schemaPath: '#/properties/entries/type',
                      keyword: 'type',
                      params: { type: 'array' },
                      message: 'must be array',
                    },
                  ];
                  return false;
                }
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.incomplete !== undefined) {
                const _errs7 = errors;
                if (typeof data.incomplete !== 'boolean') {
                  validate99.errors = [
                    {
                      instancePath: instancePath + '/incomplete',
                      schemaPath: '#/properties/incomplete/type',
                      keyword: 'type',
                      params: { type: 'boolean' },
                      message: 'must be boolean',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs7 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate99.errors = [
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
  validate99.errors = vErrors;
  return errors === 0;
}
validate99.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema44 = {
  additionalProperties: false,
  properties: {
    missing_ids: { items: { $ref: '#/$defs/MemoryId' }, type: 'array' },
    notes: { items: { $ref: '#/$defs/MemoryRead' }, type: 'array' },
  },
  required: ['notes', 'missing_ids'],
  type: 'object',
};
const schema45 = {
  additionalProperties: false,
  properties: {
    hash: { type: 'string' },
    history: { items: { $ref: '#/$defs/MemoryHistory' }, type: 'array' },
    note: { $ref: '#/$defs/MemoryDocument' },
    path: { type: 'string' },
    provenance: { anyOf: [{ $ref: '#/$defs/MemoryProvenance' }, { type: 'null' }] },
  },
  required: ['path', 'note', 'hash', 'history'],
  type: 'object',
};
const schema46 = {
  additionalProperties: false,
  properties: { body: { type: 'string' }, metadata: { $ref: '#/$defs/MemoryMetadata' } },
  required: ['metadata', 'body'],
  type: 'object',
};
const schema47 = {
  additionalProperties: false,
  properties: {
    category: { type: 'string' },
    confidence: { type: 'number' },
    conflicts_with_ids: { items: { $ref: '#/$defs/MemoryId' }, type: 'array' },
    created_at: { $ref: '#/$defs/UtcTimestamp' },
    id: { $ref: '#/$defs/MemoryId' },
    importance: { maximum: 255, minimum: 0, type: 'integer' },
    last_confirmed_at: { anyOf: [{ $ref: '#/$defs/UtcTimestamp' }, { type: 'null' }] },
    recall_weight: { type: 'number' },
    related_ids: { items: { $ref: '#/$defs/MemoryId' }, type: 'array' },
    schema_version: { minimum: 0, type: 'integer', maximum: 4294967295 },
    scope: { $ref: '#/$defs/MemoryScope' },
    source_events: { items: { type: 'string' }, type: 'array' },
    source_sessions: { items: { $ref: '#/$defs/SessionId' }, type: 'array' },
    status: { $ref: '#/$defs/MemoryStatus' },
    supersedes_ids: { items: { $ref: '#/$defs/MemoryId' }, type: 'array' },
    tags: { items: { type: 'string' }, type: 'array' },
    updated_at: { $ref: '#/$defs/UtcTimestamp' },
  },
  required: [
    'schema_version',
    'id',
    'scope',
    'category',
    'status',
    'confidence',
    'importance',
    'recall_weight',
    'created_at',
    'updated_at',
    'source_sessions',
    'source_events',
    'tags',
    'related_ids',
    'supersedes_ids',
    'conflicts_with_ids',
  ],
  type: 'object',
};
function validate122(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate122.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate122.errors = [
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
  if (!(data === 'global' || data === 'project')) {
    validate122.errors = [
      {
        instancePath,
        schemaPath: '#/enum',
        keyword: 'enum',
        params: { allowedValues: schema25.enum },
        message: 'must be equal to one of the allowed values',
      },
    ];
    return false;
  }
  validate122.errors = vErrors;
  return errors === 0;
}
validate122.evaluated = { dynamicProps: false, dynamicItems: false };
const schema49 = {
  enum: ['active', 'superseded', 'forgotten', 'conflicted', 'archived'],
  type: 'string',
};
function validate125(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate125.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate125.errors = [
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
    data === 'active' ||
    data === 'superseded' ||
    data === 'forgotten' ||
    data === 'conflicted' ||
    data === 'archived'
  )) {
    validate125.errors = [
      {
        instancePath,
        schemaPath: '#/enum',
        keyword: 'enum',
        params: { allowedValues: schema49.enum },
        message: 'must be equal to one of the allowed values',
      },
    ];
    return false;
  }
  validate125.errors = vErrors;
  return errors === 0;
}
validate125.evaluated = { dynamicProps: false, dynamicItems: false };
function validate116(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate116.evaluated;
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
        (data.schema_version === undefined && (missing0 = 'schema_version')) ||
        (data.id === undefined && (missing0 = 'id')) ||
        (data.scope === undefined && (missing0 = 'scope')) ||
        (data.category === undefined && (missing0 = 'category')) ||
        (data.status === undefined && (missing0 = 'status')) ||
        (data.confidence === undefined && (missing0 = 'confidence')) ||
        (data.importance === undefined && (missing0 = 'importance')) ||
        (data.recall_weight === undefined && (missing0 = 'recall_weight')) ||
        (data.created_at === undefined && (missing0 = 'created_at')) ||
        (data.updated_at === undefined && (missing0 = 'updated_at')) ||
        (data.source_sessions === undefined && (missing0 = 'source_sessions')) ||
        (data.source_events === undefined && (missing0 = 'source_events')) ||
        (data.tags === undefined && (missing0 = 'tags')) ||
        (data.related_ids === undefined && (missing0 = 'related_ids')) ||
        (data.supersedes_ids === undefined && (missing0 = 'supersedes_ids')) ||
        (data.conflicts_with_ids === undefined && (missing0 = 'conflicts_with_ids'))
      ) {
        validate116.errors = [
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
          if (!func7.call(schema47.properties, key0)) {
            validate116.errors = [
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
          if (data.category !== undefined) {
            const _errs2 = errors;
            if (typeof data.category !== 'string') {
              validate116.errors = [
                {
                  instancePath: instancePath + '/category',
                  schemaPath: '#/properties/category/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.confidence !== undefined) {
              let data1 = data.confidence;
              const _errs4 = errors;
              if (!(typeof data1 == 'number' && isFinite(data1))) {
                validate116.errors = [
                  {
                    instancePath: instancePath + '/confidence',
                    schemaPath: '#/properties/confidence/type',
                    keyword: 'type',
                    params: { type: 'number' },
                    message: 'must be number',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.conflicts_with_ids !== undefined) {
                let data2 = data.conflicts_with_ids;
                const _errs6 = errors;
                if (errors === _errs6) {
                  if (Array.isArray(data2)) {
                    var valid1 = true;
                    const len0 = data2.length;
                    for (let i0 = 0; i0 < len0; i0++) {
                      const _errs8 = errors;
                      if (
                        !validate104(data2[i0], {
                          instancePath: instancePath + '/conflicts_with_ids/' + i0,
                          parentData: data2,
                          parentDataProperty: i0,
                          rootData,
                          dynamicAnchors,
                        })
                      ) {
                        vErrors =
                          vErrors === null
                            ? validate104.errors
                            : vErrors.concat(validate104.errors);
                        errors = vErrors.length;
                      }
                      var valid1 = _errs8 === errors;
                      if (!valid1) {
                        break;
                      }
                    }
                  } else {
                    validate116.errors = [
                      {
                        instancePath: instancePath + '/conflicts_with_ids',
                        schemaPath: '#/properties/conflicts_with_ids/type',
                        keyword: 'type',
                        params: { type: 'array' },
                        message: 'must be array',
                      },
                    ];
                    return false;
                  }
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.created_at !== undefined) {
                  const _errs9 = errors;
                  if (
                    !validate102(data.created_at, {
                      instancePath: instancePath + '/created_at',
                      parentData: data,
                      parentDataProperty: 'created_at',
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate102.errors : vErrors.concat(validate102.errors);
                    errors = vErrors.length;
                  }
                  var valid0 = _errs9 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.id !== undefined) {
                    const _errs10 = errors;
                    if (
                      !validate104(data.id, {
                        instancePath: instancePath + '/id',
                        parentData: data,
                        parentDataProperty: 'id',
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate104.errors : vErrors.concat(validate104.errors);
                      errors = vErrors.length;
                    }
                    var valid0 = _errs10 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.importance !== undefined) {
                      let data6 = data.importance;
                      const _errs11 = errors;
                      if (!(
                        typeof data6 == 'number' &&
                        !(data6 % 1) &&
                        !isNaN(data6) &&
                        isFinite(data6)
                      )) {
                        validate116.errors = [
                          {
                            instancePath: instancePath + '/importance',
                            schemaPath: '#/properties/importance/type',
                            keyword: 'type',
                            params: { type: 'integer' },
                            message: 'must be integer',
                          },
                        ];
                        return false;
                      }
                      if (errors === _errs11) {
                        if (typeof data6 == 'number' && isFinite(data6)) {
                          if (data6 > 255 || isNaN(data6)) {
                            validate116.errors = [
                              {
                                instancePath: instancePath + '/importance',
                                schemaPath: '#/properties/importance/maximum',
                                keyword: 'maximum',
                                params: { comparison: '<=', limit: 255 },
                                message: 'must be <= 255',
                              },
                            ];
                            return false;
                          } else {
                            if (data6 < 0 || isNaN(data6)) {
                              validate116.errors = [
                                {
                                  instancePath: instancePath + '/importance',
                                  schemaPath: '#/properties/importance/minimum',
                                  keyword: 'minimum',
                                  params: { comparison: '>=', limit: 0 },
                                  message: 'must be >= 0',
                                },
                              ];
                              return false;
                            }
                          }
                        }
                      }
                      var valid0 = _errs11 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.last_confirmed_at !== undefined) {
                        let data7 = data.last_confirmed_at;
                        const _errs13 = errors;
                        const _errs14 = errors;
                        let valid2 = false;
                        const _errs15 = errors;
                        if (
                          !validate102(data7, {
                            instancePath: instancePath + '/last_confirmed_at',
                            parentData: data,
                            parentDataProperty: 'last_confirmed_at',
                            rootData,
                            dynamicAnchors,
                          })
                        ) {
                          vErrors =
                            vErrors === null
                              ? validate102.errors
                              : vErrors.concat(validate102.errors);
                          errors = vErrors.length;
                        }
                        var _valid0 = _errs15 === errors;
                        valid2 = valid2 || _valid0;
                        const _errs16 = errors;
                        if (data7 !== null) {
                          const err0 = {
                            instancePath: instancePath + '/last_confirmed_at',
                            schemaPath: '#/properties/last_confirmed_at/anyOf/1/type',
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
                        var _valid0 = _errs16 === errors;
                        valid2 = valid2 || _valid0;
                        if (!valid2) {
                          const err1 = {
                            instancePath: instancePath + '/last_confirmed_at',
                            schemaPath: '#/properties/last_confirmed_at/anyOf',
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
                          validate116.errors = vErrors;
                          return false;
                        } else {
                          errors = _errs14;
                          if (vErrors !== null) {
                            if (_errs14) {
                              vErrors.length = _errs14;
                            } else {
                              vErrors = null;
                            }
                          }
                        }
                        var valid0 = _errs13 === errors;
                      } else {
                        var valid0 = true;
                      }
                      if (valid0) {
                        if (data.recall_weight !== undefined) {
                          let data8 = data.recall_weight;
                          const _errs18 = errors;
                          if (!(typeof data8 == 'number' && isFinite(data8))) {
                            validate116.errors = [
                              {
                                instancePath: instancePath + '/recall_weight',
                                schemaPath: '#/properties/recall_weight/type',
                                keyword: 'type',
                                params: { type: 'number' },
                                message: 'must be number',
                              },
                            ];
                            return false;
                          }
                          var valid0 = _errs18 === errors;
                        } else {
                          var valid0 = true;
                        }
                        if (valid0) {
                          if (data.related_ids !== undefined) {
                            let data9 = data.related_ids;
                            const _errs20 = errors;
                            if (errors === _errs20) {
                              if (Array.isArray(data9)) {
                                var valid3 = true;
                                const len1 = data9.length;
                                for (let i1 = 0; i1 < len1; i1++) {
                                  const _errs22 = errors;
                                  if (
                                    !validate104(data9[i1], {
                                      instancePath: instancePath + '/related_ids/' + i1,
                                      parentData: data9,
                                      parentDataProperty: i1,
                                      rootData,
                                      dynamicAnchors,
                                    })
                                  ) {
                                    vErrors =
                                      vErrors === null
                                        ? validate104.errors
                                        : vErrors.concat(validate104.errors);
                                    errors = vErrors.length;
                                  }
                                  var valid3 = _errs22 === errors;
                                  if (!valid3) {
                                    break;
                                  }
                                }
                              } else {
                                validate116.errors = [
                                  {
                                    instancePath: instancePath + '/related_ids',
                                    schemaPath: '#/properties/related_ids/type',
                                    keyword: 'type',
                                    params: { type: 'array' },
                                    message: 'must be array',
                                  },
                                ];
                                return false;
                              }
                            }
                            var valid0 = _errs20 === errors;
                          } else {
                            var valid0 = true;
                          }
                          if (valid0) {
                            if (data.schema_version !== undefined) {
                              let data11 = data.schema_version;
                              const _errs23 = errors;
                              if (!(
                                typeof data11 == 'number' &&
                                !(data11 % 1) &&
                                !isNaN(data11) &&
                                isFinite(data11)
                              )) {
                                validate116.errors = [
                                  {
                                    instancePath: instancePath + '/schema_version',
                                    schemaPath: '#/properties/schema_version/type',
                                    keyword: 'type',
                                    params: { type: 'integer' },
                                    message: 'must be integer',
                                  },
                                ];
                                return false;
                              }
                              if (errors === _errs23) {
                                if (typeof data11 == 'number' && isFinite(data11)) {
                                  if (data11 > 4294967295 || isNaN(data11)) {
                                    validate116.errors = [
                                      {
                                        instancePath: instancePath + '/schema_version',
                                        schemaPath: '#/properties/schema_version/maximum',
                                        keyword: 'maximum',
                                        params: { comparison: '<=', limit: 4294967295 },
                                        message: 'must be <= 4294967295',
                                      },
                                    ];
                                    return false;
                                  } else {
                                    if (data11 < 0 || isNaN(data11)) {
                                      validate116.errors = [
                                        {
                                          instancePath: instancePath + '/schema_version',
                                          schemaPath: '#/properties/schema_version/minimum',
                                          keyword: 'minimum',
                                          params: { comparison: '>=', limit: 0 },
                                          message: 'must be >= 0',
                                        },
                                      ];
                                      return false;
                                    }
                                  }
                                }
                              }
                              var valid0 = _errs23 === errors;
                            } else {
                              var valid0 = true;
                            }
                            if (valid0) {
                              if (data.scope !== undefined) {
                                const _errs25 = errors;
                                if (
                                  !validate122(data.scope, {
                                    instancePath: instancePath + '/scope',
                                    parentData: data,
                                    parentDataProperty: 'scope',
                                    rootData,
                                    dynamicAnchors,
                                  })
                                ) {
                                  vErrors =
                                    vErrors === null
                                      ? validate122.errors
                                      : vErrors.concat(validate122.errors);
                                  errors = vErrors.length;
                                }
                                var valid0 = _errs25 === errors;
                              } else {
                                var valid0 = true;
                              }
                              if (valid0) {
                                if (data.source_events !== undefined) {
                                  let data13 = data.source_events;
                                  const _errs26 = errors;
                                  if (errors === _errs26) {
                                    if (Array.isArray(data13)) {
                                      var valid4 = true;
                                      const len2 = data13.length;
                                      for (let i2 = 0; i2 < len2; i2++) {
                                        const _errs28 = errors;
                                        if (typeof data13[i2] !== 'string') {
                                          validate116.errors = [
                                            {
                                              instancePath: instancePath + '/source_events/' + i2,
                                              schemaPath: '#/properties/source_events/items/type',
                                              keyword: 'type',
                                              params: { type: 'string' },
                                              message: 'must be string',
                                            },
                                          ];
                                          return false;
                                        }
                                        var valid4 = _errs28 === errors;
                                        if (!valid4) {
                                          break;
                                        }
                                      }
                                    } else {
                                      validate116.errors = [
                                        {
                                          instancePath: instancePath + '/source_events',
                                          schemaPath: '#/properties/source_events/type',
                                          keyword: 'type',
                                          params: { type: 'array' },
                                          message: 'must be array',
                                        },
                                      ];
                                      return false;
                                    }
                                  }
                                  var valid0 = _errs26 === errors;
                                } else {
                                  var valid0 = true;
                                }
                                if (valid0) {
                                  if (data.source_sessions !== undefined) {
                                    let data15 = data.source_sessions;
                                    const _errs30 = errors;
                                    if (errors === _errs30) {
                                      if (Array.isArray(data15)) {
                                        var valid5 = true;
                                        const len3 = data15.length;
                                        for (let i3 = 0; i3 < len3; i3++) {
                                          const _errs32 = errors;
                                          if (
                                            !validate106(data15[i3], {
                                              instancePath: instancePath + '/source_sessions/' + i3,
                                              parentData: data15,
                                              parentDataProperty: i3,
                                              rootData,
                                              dynamicAnchors,
                                            })
                                          ) {
                                            vErrors =
                                              vErrors === null
                                                ? validate106.errors
                                                : vErrors.concat(validate106.errors);
                                            errors = vErrors.length;
                                          }
                                          var valid5 = _errs32 === errors;
                                          if (!valid5) {
                                            break;
                                          }
                                        }
                                      } else {
                                        validate116.errors = [
                                          {
                                            instancePath: instancePath + '/source_sessions',
                                            schemaPath: '#/properties/source_sessions/type',
                                            keyword: 'type',
                                            params: { type: 'array' },
                                            message: 'must be array',
                                          },
                                        ];
                                        return false;
                                      }
                                    }
                                    var valid0 = _errs30 === errors;
                                  } else {
                                    var valid0 = true;
                                  }
                                  if (valid0) {
                                    if (data.status !== undefined) {
                                      const _errs33 = errors;
                                      if (
                                        !validate125(data.status, {
                                          instancePath: instancePath + '/status',
                                          parentData: data,
                                          parentDataProperty: 'status',
                                          rootData,
                                          dynamicAnchors,
                                        })
                                      ) {
                                        vErrors =
                                          vErrors === null
                                            ? validate125.errors
                                            : vErrors.concat(validate125.errors);
                                        errors = vErrors.length;
                                      }
                                      var valid0 = _errs33 === errors;
                                    } else {
                                      var valid0 = true;
                                    }
                                    if (valid0) {
                                      if (data.supersedes_ids !== undefined) {
                                        let data18 = data.supersedes_ids;
                                        const _errs34 = errors;
                                        if (errors === _errs34) {
                                          if (Array.isArray(data18)) {
                                            var valid6 = true;
                                            const len4 = data18.length;
                                            for (let i4 = 0; i4 < len4; i4++) {
                                              const _errs36 = errors;
                                              if (
                                                !validate104(data18[i4], {
                                                  instancePath:
                                                    instancePath + '/supersedes_ids/' + i4,
                                                  parentData: data18,
                                                  parentDataProperty: i4,
                                                  rootData,
                                                  dynamicAnchors,
                                                })
                                              ) {
                                                vErrors =
                                                  vErrors === null
                                                    ? validate104.errors
                                                    : vErrors.concat(validate104.errors);
                                                errors = vErrors.length;
                                              }
                                              var valid6 = _errs36 === errors;
                                              if (!valid6) {
                                                break;
                                              }
                                            }
                                          } else {
                                            validate116.errors = [
                                              {
                                                instancePath: instancePath + '/supersedes_ids',
                                                schemaPath: '#/properties/supersedes_ids/type',
                                                keyword: 'type',
                                                params: { type: 'array' },
                                                message: 'must be array',
                                              },
                                            ];
                                            return false;
                                          }
                                        }
                                        var valid0 = _errs34 === errors;
                                      } else {
                                        var valid0 = true;
                                      }
                                      if (valid0) {
                                        if (data.tags !== undefined) {
                                          let data20 = data.tags;
                                          const _errs37 = errors;
                                          if (errors === _errs37) {
                                            if (Array.isArray(data20)) {
                                              var valid7 = true;
                                              const len5 = data20.length;
                                              for (let i5 = 0; i5 < len5; i5++) {
                                                const _errs39 = errors;
                                                if (typeof data20[i5] !== 'string') {
                                                  validate116.errors = [
                                                    {
                                                      instancePath: instancePath + '/tags/' + i5,
                                                      schemaPath: '#/properties/tags/items/type',
                                                      keyword: 'type',
                                                      params: { type: 'string' },
                                                      message: 'must be string',
                                                    },
                                                  ];
                                                  return false;
                                                }
                                                var valid7 = _errs39 === errors;
                                                if (!valid7) {
                                                  break;
                                                }
                                              }
                                            } else {
                                              validate116.errors = [
                                                {
                                                  instancePath: instancePath + '/tags',
                                                  schemaPath: '#/properties/tags/type',
                                                  keyword: 'type',
                                                  params: { type: 'array' },
                                                  message: 'must be array',
                                                },
                                              ];
                                              return false;
                                            }
                                          }
                                          var valid0 = _errs37 === errors;
                                        } else {
                                          var valid0 = true;
                                        }
                                        if (valid0) {
                                          if (data.updated_at !== undefined) {
                                            const _errs41 = errors;
                                            if (
                                              !validate102(data.updated_at, {
                                                instancePath: instancePath + '/updated_at',
                                                parentData: data,
                                                parentDataProperty: 'updated_at',
                                                rootData,
                                                dynamicAnchors,
                                              })
                                            ) {
                                              vErrors =
                                                vErrors === null
                                                  ? validate102.errors
                                                  : vErrors.concat(validate102.errors);
                                              errors = vErrors.length;
                                            }
                                            var valid0 = _errs41 === errors;
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
            }
          }
        }
      }
    } else {
      validate116.errors = [
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
  validate116.errors = vErrors;
  return errors === 0;
}
validate116.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
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
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.metadata === undefined && (missing0 = 'metadata')) ||
        (data.body === undefined && (missing0 = 'body'))
      ) {
        validate115.errors = [
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
          if (!(key0 === 'body' || key0 === 'metadata')) {
            validate115.errors = [
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
          if (data.body !== undefined) {
            const _errs2 = errors;
            if (typeof data.body !== 'string') {
              validate115.errors = [
                {
                  instancePath: instancePath + '/body',
                  schemaPath: '#/properties/body/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.metadata !== undefined) {
              const _errs4 = errors;
              if (
                !validate116(data.metadata, {
                  instancePath: instancePath + '/metadata',
                  parentData: data,
                  parentDataProperty: 'metadata',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate116.errors : vErrors.concat(validate116.errors);
                errors = vErrors.length;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate115.errors = [
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
  validate115.errors = vErrors;
  return errors === 0;
}
validate115.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema50 = {
  additionalProperties: false,
  properties: {
    creator: {
      anyOf: [{ $ref: '#/$defs/MemoryAuthorStamp' }, { type: 'null' }],
      description:
        'None means there is no reliable creation record. Source sessions alone are not proof.',
    },
    modifiers: {
      description: 'New authorship stamps plus recent known events, most recent last.',
      items: { $ref: '#/$defs/MemoryAuthorStamp' },
      type: 'array',
    },
  },
  required: ['modifiers'],
  type: 'object',
};
const schema51 = {
  additionalProperties: false,
  properties: {
    actor: { type: 'string' },
    at: { $ref: '#/$defs/UtcTimestamp' },
    session_id: { anyOf: [{ $ref: '#/$defs/SessionId' }, { type: 'null' }] },
    session_name_at_write: { type: ['string', 'null'] },
  },
  required: ['actor', 'at'],
  type: 'object',
};
function validate132(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate132.evaluated;
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
        (data.actor === undefined && (missing0 = 'actor')) ||
        (data.at === undefined && (missing0 = 'at'))
      ) {
        validate132.errors = [
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
            key0 === 'actor' ||
            key0 === 'at' ||
            key0 === 'session_id' ||
            key0 === 'session_name_at_write'
          )) {
            validate132.errors = [
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
          if (data.actor !== undefined) {
            const _errs2 = errors;
            if (typeof data.actor !== 'string') {
              validate132.errors = [
                {
                  instancePath: instancePath + '/actor',
                  schemaPath: '#/properties/actor/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.at !== undefined) {
              const _errs4 = errors;
              if (
                !validate102(data.at, {
                  instancePath: instancePath + '/at',
                  parentData: data,
                  parentDataProperty: 'at',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate102.errors : vErrors.concat(validate102.errors);
                errors = vErrors.length;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.session_id !== undefined) {
                let data2 = data.session_id;
                const _errs5 = errors;
                const _errs6 = errors;
                let valid1 = false;
                const _errs7 = errors;
                if (
                  !validate106(data2, {
                    instancePath: instancePath + '/session_id',
                    parentData: data,
                    parentDataProperty: 'session_id',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate106.errors : vErrors.concat(validate106.errors);
                  errors = vErrors.length;
                }
                var _valid0 = _errs7 === errors;
                valid1 = valid1 || _valid0;
                const _errs8 = errors;
                if (data2 !== null) {
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
                  validate132.errors = vErrors;
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
                if (data.session_name_at_write !== undefined) {
                  let data3 = data.session_name_at_write;
                  const _errs10 = errors;
                  if (typeof data3 !== 'string' && data3 !== null) {
                    validate132.errors = [
                      {
                        instancePath: instancePath + '/session_name_at_write',
                        schemaPath: '#/properties/session_name_at_write/type',
                        keyword: 'type',
                        params: { type: schema51.properties.session_name_at_write.type },
                        message: 'must be string,null',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs10 === errors;
                } else {
                  var valid0 = true;
                }
              }
            }
          }
        }
      }
    } else {
      validate132.errors = [
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
  validate132.errors = vErrors;
  return errors === 0;
}
validate132.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate131(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate131.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (data.modifiers === undefined && (missing0 = 'modifiers')) {
        validate131.errors = [
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
          if (!(key0 === 'creator' || key0 === 'modifiers')) {
            validate131.errors = [
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
          if (data.creator !== undefined) {
            let data0 = data.creator;
            const _errs2 = errors;
            const _errs3 = errors;
            let valid1 = false;
            const _errs4 = errors;
            if (
              !validate132(data0, {
                instancePath: instancePath + '/creator',
                parentData: data,
                parentDataProperty: 'creator',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate132.errors : vErrors.concat(validate132.errors);
              errors = vErrors.length;
            }
            var _valid0 = _errs4 === errors;
            valid1 = valid1 || _valid0;
            const _errs5 = errors;
            if (data0 !== null) {
              const err0 = {
                instancePath: instancePath + '/creator',
                schemaPath: '#/properties/creator/anyOf/1/type',
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
            var _valid0 = _errs5 === errors;
            valid1 = valid1 || _valid0;
            if (!valid1) {
              const err1 = {
                instancePath: instancePath + '/creator',
                schemaPath: '#/properties/creator/anyOf',
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
              validate131.errors = vErrors;
              return false;
            } else {
              errors = _errs3;
              if (vErrors !== null) {
                if (_errs3) {
                  vErrors.length = _errs3;
                } else {
                  vErrors = null;
                }
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.modifiers !== undefined) {
              let data1 = data.modifiers;
              const _errs7 = errors;
              if (errors === _errs7) {
                if (Array.isArray(data1)) {
                  var valid2 = true;
                  const len0 = data1.length;
                  for (let i0 = 0; i0 < len0; i0++) {
                    const _errs9 = errors;
                    if (
                      !validate132(data1[i0], {
                        instancePath: instancePath + '/modifiers/' + i0,
                        parentData: data1,
                        parentDataProperty: i0,
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate132.errors : vErrors.concat(validate132.errors);
                      errors = vErrors.length;
                    }
                    var valid2 = _errs9 === errors;
                    if (!valid2) {
                      break;
                    }
                  }
                } else {
                  validate131.errors = [
                    {
                      instancePath: instancePath + '/modifiers',
                      schemaPath: '#/properties/modifiers/type',
                      keyword: 'type',
                      params: { type: 'array' },
                      message: 'must be array',
                    },
                  ];
                  return false;
                }
              }
              var valid0 = _errs7 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate131.errors = [
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
  validate131.errors = vErrors;
  return errors === 0;
}
validate131.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
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
      if (
        (data.path === undefined && (missing0 = 'path')) ||
        (data.note === undefined && (missing0 = 'note')) ||
        (data.hash === undefined && (missing0 = 'hash')) ||
        (data.history === undefined && (missing0 = 'history'))
      ) {
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
          if (!(
            key0 === 'hash' ||
            key0 === 'history' ||
            key0 === 'note' ||
            key0 === 'path' ||
            key0 === 'provenance'
          )) {
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
          if (data.hash !== undefined) {
            const _errs2 = errors;
            if (typeof data.hash !== 'string') {
              validate113.errors = [
                {
                  instancePath: instancePath + '/hash',
                  schemaPath: '#/properties/hash/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.history !== undefined) {
              let data1 = data.history;
              const _errs4 = errors;
              if (errors === _errs4) {
                if (Array.isArray(data1)) {
                  var valid1 = true;
                  const len0 = data1.length;
                  for (let i0 = 0; i0 < len0; i0++) {
                    const _errs6 = errors;
                    if (
                      !validate101(data1[i0], {
                        instancePath: instancePath + '/history/' + i0,
                        parentData: data1,
                        parentDataProperty: i0,
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate101.errors : vErrors.concat(validate101.errors);
                      errors = vErrors.length;
                    }
                    var valid1 = _errs6 === errors;
                    if (!valid1) {
                      break;
                    }
                  }
                } else {
                  validate113.errors = [
                    {
                      instancePath: instancePath + '/history',
                      schemaPath: '#/properties/history/type',
                      keyword: 'type',
                      params: { type: 'array' },
                      message: 'must be array',
                    },
                  ];
                  return false;
                }
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.note !== undefined) {
                const _errs7 = errors;
                if (
                  !validate115(data.note, {
                    instancePath: instancePath + '/note',
                    parentData: data,
                    parentDataProperty: 'note',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate115.errors : vErrors.concat(validate115.errors);
                  errors = vErrors.length;
                }
                var valid0 = _errs7 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.path !== undefined) {
                  const _errs8 = errors;
                  if (typeof data.path !== 'string') {
                    validate113.errors = [
                      {
                        instancePath: instancePath + '/path',
                        schemaPath: '#/properties/path/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.provenance !== undefined) {
                    let data5 = data.provenance;
                    const _errs10 = errors;
                    const _errs11 = errors;
                    let valid2 = false;
                    const _errs12 = errors;
                    if (
                      !validate131(data5, {
                        instancePath: instancePath + '/provenance',
                        parentData: data,
                        parentDataProperty: 'provenance',
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate131.errors : vErrors.concat(validate131.errors);
                      errors = vErrors.length;
                    }
                    var _valid0 = _errs12 === errors;
                    valid2 = valid2 || _valid0;
                    const _errs13 = errors;
                    if (data5 !== null) {
                      const err0 = {
                        instancePath: instancePath + '/provenance',
                        schemaPath: '#/properties/provenance/anyOf/1/type',
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
                    var _valid0 = _errs13 === errors;
                    valid2 = valid2 || _valid0;
                    if (!valid2) {
                      const err1 = {
                        instancePath: instancePath + '/provenance',
                        schemaPath: '#/properties/provenance/anyOf',
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
                      validate113.errors = vErrors;
                      return false;
                    } else {
                      errors = _errs11;
                      if (vErrors !== null) {
                        if (_errs11) {
                          vErrors.length = _errs11;
                        } else {
                          vErrors = null;
                        }
                      }
                    }
                    var valid0 = _errs10 === errors;
                  } else {
                    var valid0 = true;
                  }
                }
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
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.notes === undefined && (missing0 = 'notes')) ||
        (data.missing_ids === undefined && (missing0 = 'missing_ids'))
      ) {
        validate111.errors = [
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
          if (!(key0 === 'missing_ids' || key0 === 'notes')) {
            validate111.errors = [
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
          if (data.missing_ids !== undefined) {
            let data0 = data.missing_ids;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  const _errs4 = errors;
                  if (
                    !validate104(data0[i0], {
                      instancePath: instancePath + '/missing_ids/' + i0,
                      parentData: data0,
                      parentDataProperty: i0,
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate104.errors : vErrors.concat(validate104.errors);
                    errors = vErrors.length;
                  }
                  var valid1 = _errs4 === errors;
                  if (!valid1) {
                    break;
                  }
                }
              } else {
                validate111.errors = [
                  {
                    instancePath: instancePath + '/missing_ids',
                    schemaPath: '#/properties/missing_ids/type',
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
            if (data.notes !== undefined) {
              let data2 = data.notes;
              const _errs5 = errors;
              if (errors === _errs5) {
                if (Array.isArray(data2)) {
                  var valid2 = true;
                  const len1 = data2.length;
                  for (let i1 = 0; i1 < len1; i1++) {
                    const _errs7 = errors;
                    if (
                      !validate113(data2[i1], {
                        instancePath: instancePath + '/notes/' + i1,
                        parentData: data2,
                        parentDataProperty: i1,
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate113.errors : vErrors.concat(validate113.errors);
                      errors = vErrors.length;
                    }
                    var valid2 = _errs7 === errors;
                    if (!valid2) {
                      break;
                    }
                  }
                } else {
                  validate111.errors = [
                    {
                      instancePath: instancePath + '/notes',
                      schemaPath: '#/properties/notes/type',
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
      validate111.errors = [
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
  validate111.errors = vErrors;
  return errors === 0;
}
validate111.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema52 = {
  additionalProperties: false,
  properties: {
    recent_runs: { items: { $ref: '#/$defs/MemoryMaintenanceRun' }, type: 'array' },
    vaults: { items: { $ref: '#/$defs/MemoryVaultStatus' }, type: 'array' },
  },
  required: ['vaults', 'recent_runs'],
  type: 'object',
};
const schema53 = {
  additionalProperties: false,
  properties: {
    finished_at: { anyOf: [{ $ref: '#/$defs/UtcTimestamp' }, { type: 'null' }] },
    input_tokens: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
    model_profile_id: { anyOf: [{ $ref: '#/$defs/ModelProfileId' }, { type: 'null' }] },
    output_tokens: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
    reason: { type: ['string', 'null'] },
    run_id: { type: 'string' },
    runtime_instance_id: { $ref: '#/$defs/RuntimeInstanceId' },
    session_id: { $ref: '#/$defs/SessionId' },
    started_at: { $ref: '#/$defs/UtcTimestamp' },
    state: { $ref: '#/$defs/MemoryMaintenanceState' },
    turn_id: { type: ['string', 'null'] },
    usage_complete: {
      default: false,
      description: 'Counts observed so far; false means final Provider usage is unconfirmed.',
      type: 'boolean',
    },
  },
  required: [
    'run_id',
    'runtime_instance_id',
    'session_id',
    'state',
    'input_tokens',
    'output_tokens',
    'started_at',
  ],
  type: 'object',
};
const schema54 = {
  type: 'string',
  minLength: 30,
  maxLength: 30,
  pattern: '^mdl_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
};
const pattern8 = new RegExp('^mdl_[0-7][0-9A-HJKMNP-TV-Z]{25}$', 'u');
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
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 30) {
        validate143.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 30 },
            message: 'must NOT have more than 30 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 30) {
          validate143.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 30 },
              message: 'must NOT have fewer than 30 characters',
            },
          ];
          return false;
        } else {
          if (!pattern8.test(data)) {
            validate143.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: '^mdl_[0-7][0-9A-HJKMNP-TV-Z]{25}$' },
                message: 'must match pattern "' + '^mdl_[0-7][0-9A-HJKMNP-TV-Z]{25}$' + '"',
              },
            ];
            return false;
          }
        }
      }
    } else {
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
  }
  validate143.errors = vErrors;
  return errors === 0;
}
validate143.evaluated = { dynamicProps: false, dynamicItems: false };
const schema55 = {
  type: 'string',
  minLength: 30,
  maxLength: 30,
  pattern: '^run_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
};
const pattern9 = new RegExp('^run_[0-7][0-9A-HJKMNP-TV-Z]{25}$', 'u');
function validate145(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate145.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 30) {
        validate145.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 30 },
            message: 'must NOT have more than 30 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 30) {
          validate145.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 30 },
              message: 'must NOT have fewer than 30 characters',
            },
          ];
          return false;
        } else {
          if (!pattern9.test(data)) {
            validate145.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: '^run_[0-7][0-9A-HJKMNP-TV-Z]{25}$' },
                message: 'must match pattern "' + '^run_[0-7][0-9A-HJKMNP-TV-Z]{25}$' + '"',
              },
            ];
            return false;
          }
        }
      }
    } else {
      validate145.errors = [
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
  validate145.errors = vErrors;
  return errors === 0;
}
validate145.evaluated = { dynamicProps: false, dynamicItems: false };
const schema56 = {
  enum: ['queued', 'running', 'completed', 'skipped', 'failed', 'interrupted'],
  type: 'string',
};
function validate149(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate149.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate149.errors = [
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
    data === 'queued' ||
    data === 'running' ||
    data === 'completed' ||
    data === 'skipped' ||
    data === 'failed' ||
    data === 'interrupted'
  )) {
    validate149.errors = [
      {
        instancePath,
        schemaPath: '#/enum',
        keyword: 'enum',
        params: { allowedValues: schema56.enum },
        message: 'must be equal to one of the allowed values',
      },
    ];
    return false;
  }
  validate149.errors = vErrors;
  return errors === 0;
}
validate149.evaluated = { dynamicProps: false, dynamicItems: false };
function validate141(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate141.evaluated;
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
        (data.run_id === undefined && (missing0 = 'run_id')) ||
        (data.runtime_instance_id === undefined && (missing0 = 'runtime_instance_id')) ||
        (data.session_id === undefined && (missing0 = 'session_id')) ||
        (data.state === undefined && (missing0 = 'state')) ||
        (data.input_tokens === undefined && (missing0 = 'input_tokens')) ||
        (data.output_tokens === undefined && (missing0 = 'output_tokens')) ||
        (data.started_at === undefined && (missing0 = 'started_at'))
      ) {
        validate141.errors = [
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
          if (!func7.call(schema53.properties, key0)) {
            validate141.errors = [
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
          if (data.finished_at !== undefined) {
            let data0 = data.finished_at;
            const _errs2 = errors;
            const _errs3 = errors;
            let valid1 = false;
            const _errs4 = errors;
            if (
              !validate102(data0, {
                instancePath: instancePath + '/finished_at',
                parentData: data,
                parentDataProperty: 'finished_at',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate102.errors : vErrors.concat(validate102.errors);
              errors = vErrors.length;
            }
            var _valid0 = _errs4 === errors;
            valid1 = valid1 || _valid0;
            const _errs5 = errors;
            if (data0 !== null) {
              const err0 = {
                instancePath: instancePath + '/finished_at',
                schemaPath: '#/properties/finished_at/anyOf/1/type',
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
            var _valid0 = _errs5 === errors;
            valid1 = valid1 || _valid0;
            if (!valid1) {
              const err1 = {
                instancePath: instancePath + '/finished_at',
                schemaPath: '#/properties/finished_at/anyOf',
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
              validate141.errors = vErrors;
              return false;
            } else {
              errors = _errs3;
              if (vErrors !== null) {
                if (_errs3) {
                  vErrors.length = _errs3;
                } else {
                  vErrors = null;
                }
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.input_tokens !== undefined) {
              let data1 = data.input_tokens;
              const _errs7 = errors;
              if (!(typeof data1 == 'number' && !(data1 % 1) && !isNaN(data1) && isFinite(data1))) {
                validate141.errors = [
                  {
                    instancePath: instancePath + '/input_tokens',
                    schemaPath: '#/properties/input_tokens/type',
                    keyword: 'type',
                    params: { type: 'integer' },
                    message: 'must be integer',
                  },
                ];
                return false;
              }
              if (errors === _errs7) {
                if (typeof data1 == 'number' && isFinite(data1)) {
                  if (data1 > 9007199254740991 || isNaN(data1)) {
                    validate141.errors = [
                      {
                        instancePath: instancePath + '/input_tokens',
                        schemaPath: '#/properties/input_tokens/maximum',
                        keyword: 'maximum',
                        params: { comparison: '<=', limit: 9007199254740991 },
                        message: 'must be <= 9007199254740991',
                      },
                    ];
                    return false;
                  } else {
                    if (data1 < 0 || isNaN(data1)) {
                      validate141.errors = [
                        {
                          instancePath: instancePath + '/input_tokens',
                          schemaPath: '#/properties/input_tokens/minimum',
                          keyword: 'minimum',
                          params: { comparison: '>=', limit: 0 },
                          message: 'must be >= 0',
                        },
                      ];
                      return false;
                    }
                  }
                }
              }
              var valid0 = _errs7 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.model_profile_id !== undefined) {
                let data2 = data.model_profile_id;
                const _errs9 = errors;
                const _errs10 = errors;
                let valid2 = false;
                const _errs11 = errors;
                if (
                  !validate143(data2, {
                    instancePath: instancePath + '/model_profile_id',
                    parentData: data,
                    parentDataProperty: 'model_profile_id',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate143.errors : vErrors.concat(validate143.errors);
                  errors = vErrors.length;
                }
                var _valid1 = _errs11 === errors;
                valid2 = valid2 || _valid1;
                const _errs12 = errors;
                if (data2 !== null) {
                  const err2 = {
                    instancePath: instancePath + '/model_profile_id',
                    schemaPath: '#/properties/model_profile_id/anyOf/1/type',
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
                var _valid1 = _errs12 === errors;
                valid2 = valid2 || _valid1;
                if (!valid2) {
                  const err3 = {
                    instancePath: instancePath + '/model_profile_id',
                    schemaPath: '#/properties/model_profile_id/anyOf',
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
                  validate141.errors = vErrors;
                  return false;
                } else {
                  errors = _errs10;
                  if (vErrors !== null) {
                    if (_errs10) {
                      vErrors.length = _errs10;
                    } else {
                      vErrors = null;
                    }
                  }
                }
                var valid0 = _errs9 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.output_tokens !== undefined) {
                  let data3 = data.output_tokens;
                  const _errs14 = errors;
                  if (!(
                    typeof data3 == 'number' &&
                    !(data3 % 1) &&
                    !isNaN(data3) &&
                    isFinite(data3)
                  )) {
                    validate141.errors = [
                      {
                        instancePath: instancePath + '/output_tokens',
                        schemaPath: '#/properties/output_tokens/type',
                        keyword: 'type',
                        params: { type: 'integer' },
                        message: 'must be integer',
                      },
                    ];
                    return false;
                  }
                  if (errors === _errs14) {
                    if (typeof data3 == 'number' && isFinite(data3)) {
                      if (data3 > 9007199254740991 || isNaN(data3)) {
                        validate141.errors = [
                          {
                            instancePath: instancePath + '/output_tokens',
                            schemaPath: '#/properties/output_tokens/maximum',
                            keyword: 'maximum',
                            params: { comparison: '<=', limit: 9007199254740991 },
                            message: 'must be <= 9007199254740991',
                          },
                        ];
                        return false;
                      } else {
                        if (data3 < 0 || isNaN(data3)) {
                          validate141.errors = [
                            {
                              instancePath: instancePath + '/output_tokens',
                              schemaPath: '#/properties/output_tokens/minimum',
                              keyword: 'minimum',
                              params: { comparison: '>=', limit: 0 },
                              message: 'must be >= 0',
                            },
                          ];
                          return false;
                        }
                      }
                    }
                  }
                  var valid0 = _errs14 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.reason !== undefined) {
                    let data4 = data.reason;
                    const _errs16 = errors;
                    if (typeof data4 !== 'string' && data4 !== null) {
                      validate141.errors = [
                        {
                          instancePath: instancePath + '/reason',
                          schemaPath: '#/properties/reason/type',
                          keyword: 'type',
                          params: { type: schema53.properties.reason.type },
                          message: 'must be string,null',
                        },
                      ];
                      return false;
                    }
                    var valid0 = _errs16 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.run_id !== undefined) {
                      const _errs18 = errors;
                      if (typeof data.run_id !== 'string') {
                        validate141.errors = [
                          {
                            instancePath: instancePath + '/run_id',
                            schemaPath: '#/properties/run_id/type',
                            keyword: 'type',
                            params: { type: 'string' },
                            message: 'must be string',
                          },
                        ];
                        return false;
                      }
                      var valid0 = _errs18 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.runtime_instance_id !== undefined) {
                        const _errs20 = errors;
                        if (
                          !validate145(data.runtime_instance_id, {
                            instancePath: instancePath + '/runtime_instance_id',
                            parentData: data,
                            parentDataProperty: 'runtime_instance_id',
                            rootData,
                            dynamicAnchors,
                          })
                        ) {
                          vErrors =
                            vErrors === null
                              ? validate145.errors
                              : vErrors.concat(validate145.errors);
                          errors = vErrors.length;
                        }
                        var valid0 = _errs20 === errors;
                      } else {
                        var valid0 = true;
                      }
                      if (valid0) {
                        if (data.session_id !== undefined) {
                          const _errs21 = errors;
                          if (
                            !validate106(data.session_id, {
                              instancePath: instancePath + '/session_id',
                              parentData: data,
                              parentDataProperty: 'session_id',
                              rootData,
                              dynamicAnchors,
                            })
                          ) {
                            vErrors =
                              vErrors === null
                                ? validate106.errors
                                : vErrors.concat(validate106.errors);
                            errors = vErrors.length;
                          }
                          var valid0 = _errs21 === errors;
                        } else {
                          var valid0 = true;
                        }
                        if (valid0) {
                          if (data.started_at !== undefined) {
                            const _errs22 = errors;
                            if (
                              !validate102(data.started_at, {
                                instancePath: instancePath + '/started_at',
                                parentData: data,
                                parentDataProperty: 'started_at',
                                rootData,
                                dynamicAnchors,
                              })
                            ) {
                              vErrors =
                                vErrors === null
                                  ? validate102.errors
                                  : vErrors.concat(validate102.errors);
                              errors = vErrors.length;
                            }
                            var valid0 = _errs22 === errors;
                          } else {
                            var valid0 = true;
                          }
                          if (valid0) {
                            if (data.state !== undefined) {
                              const _errs23 = errors;
                              if (
                                !validate149(data.state, {
                                  instancePath: instancePath + '/state',
                                  parentData: data,
                                  parentDataProperty: 'state',
                                  rootData,
                                  dynamicAnchors,
                                })
                              ) {
                                vErrors =
                                  vErrors === null
                                    ? validate149.errors
                                    : vErrors.concat(validate149.errors);
                                errors = vErrors.length;
                              }
                              var valid0 = _errs23 === errors;
                            } else {
                              var valid0 = true;
                            }
                            if (valid0) {
                              if (data.turn_id !== undefined) {
                                let data10 = data.turn_id;
                                const _errs24 = errors;
                                if (typeof data10 !== 'string' && data10 !== null) {
                                  validate141.errors = [
                                    {
                                      instancePath: instancePath + '/turn_id',
                                      schemaPath: '#/properties/turn_id/type',
                                      keyword: 'type',
                                      params: { type: schema53.properties.turn_id.type },
                                      message: 'must be string,null',
                                    },
                                  ];
                                  return false;
                                }
                                var valid0 = _errs24 === errors;
                              } else {
                                var valid0 = true;
                              }
                              if (valid0) {
                                if (data.usage_complete !== undefined) {
                                  const _errs26 = errors;
                                  if (typeof data.usage_complete !== 'boolean') {
                                    validate141.errors = [
                                      {
                                        instancePath: instancePath + '/usage_complete',
                                        schemaPath: '#/properties/usage_complete/type',
                                        keyword: 'type',
                                        params: { type: 'boolean' },
                                        message: 'must be boolean',
                                      },
                                    ];
                                    return false;
                                  }
                                  var valid0 = _errs26 === errors;
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
    } else {
      validate141.errors = [
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
  validate141.errors = vErrors;
  return errors === 0;
}
validate141.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema57 = {
  additionalProperties: false,
  properties: {
    directory: { type: 'string' },
    exists: { type: 'boolean' },
    invalid: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
    maintenance_log: {
      default: null,
      description:
        "The plain text log of this Vault's finished maintenance runs, once one was written.",
      type: ['string', 'null'],
    },
    rebuilding: { type: 'boolean' },
    recent_changes: { items: { $ref: '#/$defs/MemoryHistory' }, type: 'array' },
    scope: { $ref: '#/$defs/MemoryScope' },
    valid: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
    watcher_error: { type: ['string', 'null'] },
  },
  required: ['scope', 'directory', 'exists', 'valid', 'invalid', 'rebuilding', 'recent_changes'],
  type: 'object',
};
function validate152(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate152.evaluated;
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
        (data.scope === undefined && (missing0 = 'scope')) ||
        (data.directory === undefined && (missing0 = 'directory')) ||
        (data.exists === undefined && (missing0 = 'exists')) ||
        (data.valid === undefined && (missing0 = 'valid')) ||
        (data.invalid === undefined && (missing0 = 'invalid')) ||
        (data.rebuilding === undefined && (missing0 = 'rebuilding')) ||
        (data.recent_changes === undefined && (missing0 = 'recent_changes'))
      ) {
        validate152.errors = [
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
          if (!func7.call(schema57.properties, key0)) {
            validate152.errors = [
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
          if (data.directory !== undefined) {
            const _errs2 = errors;
            if (typeof data.directory !== 'string') {
              validate152.errors = [
                {
                  instancePath: instancePath + '/directory',
                  schemaPath: '#/properties/directory/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.exists !== undefined) {
              const _errs4 = errors;
              if (typeof data.exists !== 'boolean') {
                validate152.errors = [
                  {
                    instancePath: instancePath + '/exists',
                    schemaPath: '#/properties/exists/type',
                    keyword: 'type',
                    params: { type: 'boolean' },
                    message: 'must be boolean',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.invalid !== undefined) {
                let data2 = data.invalid;
                const _errs6 = errors;
                if (!(
                  typeof data2 == 'number' &&
                  !(data2 % 1) &&
                  !isNaN(data2) &&
                  isFinite(data2)
                )) {
                  validate152.errors = [
                    {
                      instancePath: instancePath + '/invalid',
                      schemaPath: '#/properties/invalid/type',
                      keyword: 'type',
                      params: { type: 'integer' },
                      message: 'must be integer',
                    },
                  ];
                  return false;
                }
                if (errors === _errs6) {
                  if (typeof data2 == 'number' && isFinite(data2)) {
                    if (data2 > 9007199254740991 || isNaN(data2)) {
                      validate152.errors = [
                        {
                          instancePath: instancePath + '/invalid',
                          schemaPath: '#/properties/invalid/maximum',
                          keyword: 'maximum',
                          params: { comparison: '<=', limit: 9007199254740991 },
                          message: 'must be <= 9007199254740991',
                        },
                      ];
                      return false;
                    } else {
                      if (data2 < 0 || isNaN(data2)) {
                        validate152.errors = [
                          {
                            instancePath: instancePath + '/invalid',
                            schemaPath: '#/properties/invalid/minimum',
                            keyword: 'minimum',
                            params: { comparison: '>=', limit: 0 },
                            message: 'must be >= 0',
                          },
                        ];
                        return false;
                      }
                    }
                  }
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.maintenance_log !== undefined) {
                  let data3 = data.maintenance_log;
                  const _errs8 = errors;
                  if (typeof data3 !== 'string' && data3 !== null) {
                    validate152.errors = [
                      {
                        instancePath: instancePath + '/maintenance_log',
                        schemaPath: '#/properties/maintenance_log/type',
                        keyword: 'type',
                        params: { type: schema57.properties.maintenance_log.type },
                        message: 'must be string,null',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.rebuilding !== undefined) {
                    const _errs10 = errors;
                    if (typeof data.rebuilding !== 'boolean') {
                      validate152.errors = [
                        {
                          instancePath: instancePath + '/rebuilding',
                          schemaPath: '#/properties/rebuilding/type',
                          keyword: 'type',
                          params: { type: 'boolean' },
                          message: 'must be boolean',
                        },
                      ];
                      return false;
                    }
                    var valid0 = _errs10 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.recent_changes !== undefined) {
                      let data5 = data.recent_changes;
                      const _errs12 = errors;
                      if (errors === _errs12) {
                        if (Array.isArray(data5)) {
                          var valid1 = true;
                          const len0 = data5.length;
                          for (let i0 = 0; i0 < len0; i0++) {
                            const _errs14 = errors;
                            if (
                              !validate101(data5[i0], {
                                instancePath: instancePath + '/recent_changes/' + i0,
                                parentData: data5,
                                parentDataProperty: i0,
                                rootData,
                                dynamicAnchors,
                              })
                            ) {
                              vErrors =
                                vErrors === null
                                  ? validate101.errors
                                  : vErrors.concat(validate101.errors);
                              errors = vErrors.length;
                            }
                            var valid1 = _errs14 === errors;
                            if (!valid1) {
                              break;
                            }
                          }
                        } else {
                          validate152.errors = [
                            {
                              instancePath: instancePath + '/recent_changes',
                              schemaPath: '#/properties/recent_changes/type',
                              keyword: 'type',
                              params: { type: 'array' },
                              message: 'must be array',
                            },
                          ];
                          return false;
                        }
                      }
                      var valid0 = _errs12 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.scope !== undefined) {
                        const _errs15 = errors;
                        if (
                          !validate122(data.scope, {
                            instancePath: instancePath + '/scope',
                            parentData: data,
                            parentDataProperty: 'scope',
                            rootData,
                            dynamicAnchors,
                          })
                        ) {
                          vErrors =
                            vErrors === null
                              ? validate122.errors
                              : vErrors.concat(validate122.errors);
                          errors = vErrors.length;
                        }
                        var valid0 = _errs15 === errors;
                      } else {
                        var valid0 = true;
                      }
                      if (valid0) {
                        if (data.valid !== undefined) {
                          let data8 = data.valid;
                          const _errs16 = errors;
                          if (!(
                            typeof data8 == 'number' &&
                            !(data8 % 1) &&
                            !isNaN(data8) &&
                            isFinite(data8)
                          )) {
                            validate152.errors = [
                              {
                                instancePath: instancePath + '/valid',
                                schemaPath: '#/properties/valid/type',
                                keyword: 'type',
                                params: { type: 'integer' },
                                message: 'must be integer',
                              },
                            ];
                            return false;
                          }
                          if (errors === _errs16) {
                            if (typeof data8 == 'number' && isFinite(data8)) {
                              if (data8 > 9007199254740991 || isNaN(data8)) {
                                validate152.errors = [
                                  {
                                    instancePath: instancePath + '/valid',
                                    schemaPath: '#/properties/valid/maximum',
                                    keyword: 'maximum',
                                    params: { comparison: '<=', limit: 9007199254740991 },
                                    message: 'must be <= 9007199254740991',
                                  },
                                ];
                                return false;
                              } else {
                                if (data8 < 0 || isNaN(data8)) {
                                  validate152.errors = [
                                    {
                                      instancePath: instancePath + '/valid',
                                      schemaPath: '#/properties/valid/minimum',
                                      keyword: 'minimum',
                                      params: { comparison: '>=', limit: 0 },
                                      message: 'must be >= 0',
                                    },
                                  ];
                                  return false;
                                }
                              }
                            }
                          }
                          var valid0 = _errs16 === errors;
                        } else {
                          var valid0 = true;
                        }
                        if (valid0) {
                          if (data.watcher_error !== undefined) {
                            let data9 = data.watcher_error;
                            const _errs18 = errors;
                            if (typeof data9 !== 'string' && data9 !== null) {
                              validate152.errors = [
                                {
                                  instancePath: instancePath + '/watcher_error',
                                  schemaPath: '#/properties/watcher_error/type',
                                  keyword: 'type',
                                  params: { type: schema57.properties.watcher_error.type },
                                  message: 'must be string,null',
                                },
                              ];
                              return false;
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
    } else {
      validate152.errors = [
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
  validate152.errors = vErrors;
  return errors === 0;
}
validate152.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
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
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.vaults === undefined && (missing0 = 'vaults')) ||
        (data.recent_runs === undefined && (missing0 = 'recent_runs'))
      ) {
        validate140.errors = [
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
          if (!(key0 === 'recent_runs' || key0 === 'vaults')) {
            validate140.errors = [
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
          if (data.recent_runs !== undefined) {
            let data0 = data.recent_runs;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  const _errs4 = errors;
                  if (
                    !validate141(data0[i0], {
                      instancePath: instancePath + '/recent_runs/' + i0,
                      parentData: data0,
                      parentDataProperty: i0,
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate141.errors : vErrors.concat(validate141.errors);
                    errors = vErrors.length;
                  }
                  var valid1 = _errs4 === errors;
                  if (!valid1) {
                    break;
                  }
                }
              } else {
                validate140.errors = [
                  {
                    instancePath: instancePath + '/recent_runs',
                    schemaPath: '#/properties/recent_runs/type',
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
            if (data.vaults !== undefined) {
              let data2 = data.vaults;
              const _errs5 = errors;
              if (errors === _errs5) {
                if (Array.isArray(data2)) {
                  var valid2 = true;
                  const len1 = data2.length;
                  for (let i1 = 0; i1 < len1; i1++) {
                    const _errs7 = errors;
                    if (
                      !validate152(data2[i1], {
                        instancePath: instancePath + '/vaults/' + i1,
                        parentData: data2,
                        parentDataProperty: i1,
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate152.errors : vErrors.concat(validate152.errors);
                      errors = vErrors.length;
                    }
                    var valid2 = _errs7 === errors;
                    if (!valid2) {
                      break;
                    }
                  }
                } else {
                  validate140.errors = [
                    {
                      instancePath: instancePath + '/vaults',
                      schemaPath: '#/properties/vaults/type',
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
      validate140.errors = [
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
  validate140.errors = vErrors;
  return errors === 0;
}
validate140.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema58 = {
  additionalProperties: false,
  properties: {
    issue: { type: ['string', 'null'] },
    revision: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
    settings: { $ref: '#/$defs/MemorySettings' },
  },
  required: ['settings'],
  type: 'object',
};
function validate158(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate158.evaluated;
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
        (data.automatic_maintenance === undefined && (missing0 = 'automatic_maintenance')) ||
        (data.event_retention_days === undefined && (missing0 = 'event_retention_days'))
      ) {
        validate158.errors = [
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
          if (!(key0 === 'automatic_maintenance' || key0 === 'event_retention_days')) {
            validate158.errors = [
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
          if (data.automatic_maintenance !== undefined) {
            const _errs2 = errors;
            if (typeof data.automatic_maintenance !== 'boolean') {
              validate158.errors = [
                {
                  instancePath: instancePath + '/automatic_maintenance',
                  schemaPath: '#/properties/automatic_maintenance/type',
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
            if (data.event_retention_days !== undefined) {
              let data1 = data.event_retention_days;
              const _errs4 = errors;
              if (!(typeof data1 == 'number' && !(data1 % 1) && !isNaN(data1) && isFinite(data1))) {
                validate158.errors = [
                  {
                    instancePath: instancePath + '/event_retention_days',
                    schemaPath: '#/properties/event_retention_days/type',
                    keyword: 'type',
                    params: { type: 'integer' },
                    message: 'must be integer',
                  },
                ];
                return false;
              }
              if (errors === _errs4) {
                if (typeof data1 == 'number' && isFinite(data1)) {
                  if (data1 > 3650 || isNaN(data1)) {
                    validate158.errors = [
                      {
                        instancePath: instancePath + '/event_retention_days',
                        schemaPath: '#/properties/event_retention_days/maximum',
                        keyword: 'maximum',
                        params: { comparison: '<=', limit: 3650 },
                        message: 'must be <= 3650',
                      },
                    ];
                    return false;
                  } else {
                    if (data1 < 1 || isNaN(data1)) {
                      validate158.errors = [
                        {
                          instancePath: instancePath + '/event_retention_days',
                          schemaPath: '#/properties/event_retention_days/minimum',
                          keyword: 'minimum',
                          params: { comparison: '>=', limit: 1 },
                          message: 'must be >= 1',
                        },
                      ];
                      return false;
                    }
                  }
                }
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate158.errors = [
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
  validate158.errors = vErrors;
  return errors === 0;
}
validate158.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate157(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate157.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (data.settings === undefined && (missing0 = 'settings')) {
        validate157.errors = [
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
          if (!(key0 === 'issue' || key0 === 'revision' || key0 === 'settings')) {
            validate157.errors = [
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
          if (data.issue !== undefined) {
            let data0 = data.issue;
            const _errs2 = errors;
            if (typeof data0 !== 'string' && data0 !== null) {
              validate157.errors = [
                {
                  instancePath: instancePath + '/issue',
                  schemaPath: '#/properties/issue/type',
                  keyword: 'type',
                  params: { type: schema58.properties.issue.type },
                  message: 'must be string,null',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.revision !== undefined) {
              let data1 = data.revision;
              const _errs4 = errors;
              if (
                !(typeof data1 == 'number' && !(data1 % 1) && !isNaN(data1) && isFinite(data1)) &&
                data1 !== null
              ) {
                validate157.errors = [
                  {
                    instancePath: instancePath + '/revision',
                    schemaPath: '#/properties/revision/type',
                    keyword: 'type',
                    params: { type: schema58.properties.revision.type },
                    message: 'must be integer,null',
                  },
                ];
                return false;
              }
              if (errors === _errs4) {
                if (typeof data1 == 'number' && isFinite(data1)) {
                  if (data1 > 9007199254740991 || isNaN(data1)) {
                    validate157.errors = [
                      {
                        instancePath: instancePath + '/revision',
                        schemaPath: '#/properties/revision/maximum',
                        keyword: 'maximum',
                        params: { comparison: '<=', limit: 9007199254740991 },
                        message: 'must be <= 9007199254740991',
                      },
                    ];
                    return false;
                  } else {
                    if (data1 < 0 || isNaN(data1)) {
                      validate157.errors = [
                        {
                          instancePath: instancePath + '/revision',
                          schemaPath: '#/properties/revision/minimum',
                          keyword: 'minimum',
                          params: { comparison: '>=', limit: 0 },
                          message: 'must be >= 0',
                        },
                      ];
                      return false;
                    }
                  }
                }
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.settings !== undefined) {
                const _errs6 = errors;
                if (
                  !validate158(data.settings, {
                    instancePath: instancePath + '/settings',
                    parentData: data,
                    parentDataProperty: 'settings',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate158.errors : vErrors.concat(validate158.errors);
                  errors = vErrors.length;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate157.errors = [
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
  validate157.errors = vErrors;
  return errors === 0;
}
validate157.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema60 = {
  additionalProperties: false,
  properties: {
    invalid: { items: { $ref: '#/$defs/MemoryInvalidFile' }, type: 'array' },
    rebuilding: { type: 'boolean' },
    valid: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
  },
  required: ['valid', 'invalid', 'rebuilding'],
  type: 'object',
};
const schema61 = {
  additionalProperties: false,
  properties: { path: { type: 'string' }, reason: { type: 'string' } },
  required: ['path', 'reason'],
  type: 'object',
};
function validate163(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate163.evaluated;
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
        (data.path === undefined && (missing0 = 'path')) ||
        (data.reason === undefined && (missing0 = 'reason'))
      ) {
        validate163.errors = [
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
          if (!(key0 === 'path' || key0 === 'reason')) {
            validate163.errors = [
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
          if (data.path !== undefined) {
            const _errs2 = errors;
            if (typeof data.path !== 'string') {
              validate163.errors = [
                {
                  instancePath: instancePath + '/path',
                  schemaPath: '#/properties/path/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.reason !== undefined) {
              const _errs4 = errors;
              if (typeof data.reason !== 'string') {
                validate163.errors = [
                  {
                    instancePath: instancePath + '/reason',
                    schemaPath: '#/properties/reason/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate163.errors = [
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
  validate163.errors = vErrors;
  return errors === 0;
}
validate163.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate162(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate162.evaluated;
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
        (data.valid === undefined && (missing0 = 'valid')) ||
        (data.invalid === undefined && (missing0 = 'invalid')) ||
        (data.rebuilding === undefined && (missing0 = 'rebuilding'))
      ) {
        validate162.errors = [
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
          if (!(key0 === 'invalid' || key0 === 'rebuilding' || key0 === 'valid')) {
            validate162.errors = [
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
          if (data.invalid !== undefined) {
            let data0 = data.invalid;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  const _errs4 = errors;
                  if (
                    !validate163(data0[i0], {
                      instancePath: instancePath + '/invalid/' + i0,
                      parentData: data0,
                      parentDataProperty: i0,
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate163.errors : vErrors.concat(validate163.errors);
                    errors = vErrors.length;
                  }
                  var valid1 = _errs4 === errors;
                  if (!valid1) {
                    break;
                  }
                }
              } else {
                validate162.errors = [
                  {
                    instancePath: instancePath + '/invalid',
                    schemaPath: '#/properties/invalid/type',
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
            if (data.rebuilding !== undefined) {
              const _errs5 = errors;
              if (typeof data.rebuilding !== 'boolean') {
                validate162.errors = [
                  {
                    instancePath: instancePath + '/rebuilding',
                    schemaPath: '#/properties/rebuilding/type',
                    keyword: 'type',
                    params: { type: 'boolean' },
                    message: 'must be boolean',
                  },
                ];
                return false;
              }
              var valid0 = _errs5 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.valid !== undefined) {
                let data3 = data.valid;
                const _errs7 = errors;
                if (!(
                  typeof data3 == 'number' &&
                  !(data3 % 1) &&
                  !isNaN(data3) &&
                  isFinite(data3)
                )) {
                  validate162.errors = [
                    {
                      instancePath: instancePath + '/valid',
                      schemaPath: '#/properties/valid/type',
                      keyword: 'type',
                      params: { type: 'integer' },
                      message: 'must be integer',
                    },
                  ];
                  return false;
                }
                if (errors === _errs7) {
                  if (typeof data3 == 'number' && isFinite(data3)) {
                    if (data3 > 9007199254740991 || isNaN(data3)) {
                      validate162.errors = [
                        {
                          instancePath: instancePath + '/valid',
                          schemaPath: '#/properties/valid/maximum',
                          keyword: 'maximum',
                          params: { comparison: '<=', limit: 9007199254740991 },
                          message: 'must be <= 9007199254740991',
                        },
                      ];
                      return false;
                    } else {
                      if (data3 < 0 || isNaN(data3)) {
                        validate162.errors = [
                          {
                            instancePath: instancePath + '/valid',
                            schemaPath: '#/properties/valid/minimum',
                            keyword: 'minimum',
                            params: { comparison: '>=', limit: 0 },
                            message: 'must be >= 0',
                          },
                        ];
                        return false;
                      }
                    }
                  }
                }
                var valid0 = _errs7 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate162.errors = [
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
  validate162.errors = vErrors;
  return errors === 0;
}
validate162.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema62 = {
  additionalProperties: false,
  properties: {
    changes: { items: { $ref: '#/$defs/MemoryRepairChange' }, type: 'array' },
    manual: { items: { $ref: '#/$defs/MemoryInvalidFile' }, type: 'array' },
    preview_token: { type: 'string' },
    rebuild_index: { type: 'boolean' },
  },
  required: ['preview_token', 'changes', 'manual', 'rebuild_index'],
  type: 'object',
};
const schema63 = {
  additionalProperties: false,
  properties: {
    changes: { items: { type: 'string' }, type: 'array' },
    expected_hash: { type: ['string', 'null'] },
    path: { type: 'string' },
    proposed_hash: { type: 'string' },
  },
  required: ['path', 'proposed_hash', 'changes'],
  type: 'object',
};
function validate168(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate168.evaluated;
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
        (data.path === undefined && (missing0 = 'path')) ||
        (data.proposed_hash === undefined && (missing0 = 'proposed_hash')) ||
        (data.changes === undefined && (missing0 = 'changes'))
      ) {
        validate168.errors = [
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
            key0 === 'changes' ||
            key0 === 'expected_hash' ||
            key0 === 'path' ||
            key0 === 'proposed_hash'
          )) {
            validate168.errors = [
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
          if (data.changes !== undefined) {
            let data0 = data.changes;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  const _errs4 = errors;
                  if (typeof data0[i0] !== 'string') {
                    validate168.errors = [
                      {
                        instancePath: instancePath + '/changes/' + i0,
                        schemaPath: '#/properties/changes/items/type',
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
                validate168.errors = [
                  {
                    instancePath: instancePath + '/changes',
                    schemaPath: '#/properties/changes/type',
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
            if (data.expected_hash !== undefined) {
              let data2 = data.expected_hash;
              const _errs6 = errors;
              if (typeof data2 !== 'string' && data2 !== null) {
                validate168.errors = [
                  {
                    instancePath: instancePath + '/expected_hash',
                    schemaPath: '#/properties/expected_hash/type',
                    keyword: 'type',
                    params: { type: schema63.properties.expected_hash.type },
                    message: 'must be string,null',
                  },
                ];
                return false;
              }
              var valid0 = _errs6 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.path !== undefined) {
                const _errs8 = errors;
                if (typeof data.path !== 'string') {
                  validate168.errors = [
                    {
                      instancePath: instancePath + '/path',
                      schemaPath: '#/properties/path/type',
                      keyword: 'type',
                      params: { type: 'string' },
                      message: 'must be string',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs8 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.proposed_hash !== undefined) {
                  const _errs10 = errors;
                  if (typeof data.proposed_hash !== 'string') {
                    validate168.errors = [
                      {
                        instancePath: instancePath + '/proposed_hash',
                        schemaPath: '#/properties/proposed_hash/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs10 === errors;
                } else {
                  var valid0 = true;
                }
              }
            }
          }
        }
      }
    } else {
      validate168.errors = [
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
  validate168.errors = vErrors;
  return errors === 0;
}
validate168.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate167(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate167.evaluated;
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
        (data.preview_token === undefined && (missing0 = 'preview_token')) ||
        (data.changes === undefined && (missing0 = 'changes')) ||
        (data.manual === undefined && (missing0 = 'manual')) ||
        (data.rebuild_index === undefined && (missing0 = 'rebuild_index'))
      ) {
        validate167.errors = [
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
            key0 === 'changes' ||
            key0 === 'manual' ||
            key0 === 'preview_token' ||
            key0 === 'rebuild_index'
          )) {
            validate167.errors = [
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
          if (data.changes !== undefined) {
            let data0 = data.changes;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  const _errs4 = errors;
                  if (
                    !validate168(data0[i0], {
                      instancePath: instancePath + '/changes/' + i0,
                      parentData: data0,
                      parentDataProperty: i0,
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate168.errors : vErrors.concat(validate168.errors);
                    errors = vErrors.length;
                  }
                  var valid1 = _errs4 === errors;
                  if (!valid1) {
                    break;
                  }
                }
              } else {
                validate167.errors = [
                  {
                    instancePath: instancePath + '/changes',
                    schemaPath: '#/properties/changes/type',
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
            if (data.manual !== undefined) {
              let data2 = data.manual;
              const _errs5 = errors;
              if (errors === _errs5) {
                if (Array.isArray(data2)) {
                  var valid2 = true;
                  const len1 = data2.length;
                  for (let i1 = 0; i1 < len1; i1++) {
                    const _errs7 = errors;
                    if (
                      !validate163(data2[i1], {
                        instancePath: instancePath + '/manual/' + i1,
                        parentData: data2,
                        parentDataProperty: i1,
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate163.errors : vErrors.concat(validate163.errors);
                      errors = vErrors.length;
                    }
                    var valid2 = _errs7 === errors;
                    if (!valid2) {
                      break;
                    }
                  }
                } else {
                  validate167.errors = [
                    {
                      instancePath: instancePath + '/manual',
                      schemaPath: '#/properties/manual/type',
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
            if (valid0) {
              if (data.preview_token !== undefined) {
                const _errs8 = errors;
                if (typeof data.preview_token !== 'string') {
                  validate167.errors = [
                    {
                      instancePath: instancePath + '/preview_token',
                      schemaPath: '#/properties/preview_token/type',
                      keyword: 'type',
                      params: { type: 'string' },
                      message: 'must be string',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs8 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.rebuild_index !== undefined) {
                  const _errs10 = errors;
                  if (typeof data.rebuild_index !== 'boolean') {
                    validate167.errors = [
                      {
                        instancePath: instancePath + '/rebuild_index',
                        schemaPath: '#/properties/rebuild_index/type',
                        keyword: 'type',
                        params: { type: 'boolean' },
                        message: 'must be boolean',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs10 === errors;
                } else {
                  var valid0 = true;
                }
              }
            }
          }
        }
      }
    } else {
      validate167.errors = [
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
  validate167.errors = vErrors;
  return errors === 0;
}
validate167.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema64 = {
  additionalProperties: false,
  properties: { applied: { type: 'boolean' }, preview: { $ref: '#/$defs/MemoryRepairPreview' } },
  required: ['preview', 'applied'],
  type: 'object',
};
function validate172(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate172.evaluated;
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
        (data.preview === undefined && (missing0 = 'preview')) ||
        (data.applied === undefined && (missing0 = 'applied'))
      ) {
        validate172.errors = [
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
          if (!(key0 === 'applied' || key0 === 'preview')) {
            validate172.errors = [
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
          if (data.applied !== undefined) {
            const _errs2 = errors;
            if (typeof data.applied !== 'boolean') {
              validate172.errors = [
                {
                  instancePath: instancePath + '/applied',
                  schemaPath: '#/properties/applied/type',
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
            if (data.preview !== undefined) {
              const _errs4 = errors;
              if (
                !validate167(data.preview, {
                  instancePath: instancePath + '/preview',
                  parentData: data,
                  parentDataProperty: 'preview',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate167.errors : vErrors.concat(validate167.errors);
                errors = vErrors.length;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate172.errors = [
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
  validate172.errors = vErrors;
  return errors === 0;
}
validate172.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema65 = {
  additionalProperties: false,
  properties: { completed: { type: 'boolean' }, message: { type: 'string' } },
  required: ['completed', 'message'],
  type: 'object',
};
function validate175(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate175.evaluated;
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
        (data.completed === undefined && (missing0 = 'completed')) ||
        (data.message === undefined && (missing0 = 'message'))
      ) {
        validate175.errors = [
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
          if (!(key0 === 'completed' || key0 === 'message')) {
            validate175.errors = [
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
          if (data.completed !== undefined) {
            const _errs2 = errors;
            if (typeof data.completed !== 'boolean') {
              validate175.errors = [
                {
                  instancePath: instancePath + '/completed',
                  schemaPath: '#/properties/completed/type',
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
            if (data.message !== undefined) {
              const _errs4 = errors;
              if (typeof data.message !== 'string') {
                validate175.errors = [
                  {
                    instancePath: instancePath + '/message',
                    schemaPath: '#/properties/message/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate175.errors = [
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
  validate175.errors = vErrors;
  return errors === 0;
}
validate175.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema66 = {
  additionalProperties: false,
  properties: {
    entries: { minimum: 0, type: 'integer', maximum: 4294967295 },
    export_id: { type: 'string' },
    total_bytes: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
  },
  required: ['export_id', 'entries', 'total_bytes'],
  type: 'object',
};
function validate179(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate179.evaluated;
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
        (data.export_id === undefined && (missing0 = 'export_id')) ||
        (data.entries === undefined && (missing0 = 'entries')) ||
        (data.total_bytes === undefined && (missing0 = 'total_bytes'))
      ) {
        validate179.errors = [
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
          if (!(key0 === 'entries' || key0 === 'export_id' || key0 === 'total_bytes')) {
            validate179.errors = [
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
          if (data.entries !== undefined) {
            let data0 = data.entries;
            const _errs2 = errors;
            if (!(typeof data0 == 'number' && !(data0 % 1) && !isNaN(data0) && isFinite(data0))) {
              validate179.errors = [
                {
                  instancePath: instancePath + '/entries',
                  schemaPath: '#/properties/entries/type',
                  keyword: 'type',
                  params: { type: 'integer' },
                  message: 'must be integer',
                },
              ];
              return false;
            }
            if (errors === _errs2) {
              if (typeof data0 == 'number' && isFinite(data0)) {
                if (data0 > 4294967295 || isNaN(data0)) {
                  validate179.errors = [
                    {
                      instancePath: instancePath + '/entries',
                      schemaPath: '#/properties/entries/maximum',
                      keyword: 'maximum',
                      params: { comparison: '<=', limit: 4294967295 },
                      message: 'must be <= 4294967295',
                    },
                  ];
                  return false;
                } else {
                  if (data0 < 0 || isNaN(data0)) {
                    validate179.errors = [
                      {
                        instancePath: instancePath + '/entries',
                        schemaPath: '#/properties/entries/minimum',
                        keyword: 'minimum',
                        params: { comparison: '>=', limit: 0 },
                        message: 'must be >= 0',
                      },
                    ];
                    return false;
                  }
                }
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.export_id !== undefined) {
              const _errs4 = errors;
              if (typeof data.export_id !== 'string') {
                validate179.errors = [
                  {
                    instancePath: instancePath + '/export_id',
                    schemaPath: '#/properties/export_id/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.total_bytes !== undefined) {
                let data2 = data.total_bytes;
                const _errs6 = errors;
                if (!(
                  typeof data2 == 'number' &&
                  !(data2 % 1) &&
                  !isNaN(data2) &&
                  isFinite(data2)
                )) {
                  validate179.errors = [
                    {
                      instancePath: instancePath + '/total_bytes',
                      schemaPath: '#/properties/total_bytes/type',
                      keyword: 'type',
                      params: { type: 'integer' },
                      message: 'must be integer',
                    },
                  ];
                  return false;
                }
                if (errors === _errs6) {
                  if (typeof data2 == 'number' && isFinite(data2)) {
                    if (data2 > 9007199254740991 || isNaN(data2)) {
                      validate179.errors = [
                        {
                          instancePath: instancePath + '/total_bytes',
                          schemaPath: '#/properties/total_bytes/maximum',
                          keyword: 'maximum',
                          params: { comparison: '<=', limit: 9007199254740991 },
                          message: 'must be <= 9007199254740991',
                        },
                      ];
                      return false;
                    } else {
                      if (data2 < 0 || isNaN(data2)) {
                        validate179.errors = [
                          {
                            instancePath: instancePath + '/total_bytes',
                            schemaPath: '#/properties/total_bytes/minimum',
                            keyword: 'minimum',
                            params: { comparison: '>=', limit: 0 },
                            message: 'must be >= 0',
                          },
                        ];
                        return false;
                      }
                    }
                  }
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate179.errors = [
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
  validate179.errors = vErrors;
  return errors === 0;
}
validate179.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema67 = {
  additionalProperties: false,
  properties: {
    directory: { type: 'boolean' },
    length: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
    path: { type: 'string' },
    sha256: { type: 'string' },
  },
  required: ['path', 'directory', 'length', 'sha256'],
  type: 'object',
};
function validate181(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate181.evaluated;
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
        (data.path === undefined && (missing0 = 'path')) ||
        (data.directory === undefined && (missing0 = 'directory')) ||
        (data.length === undefined && (missing0 = 'length')) ||
        (data.sha256 === undefined && (missing0 = 'sha256'))
      ) {
        validate181.errors = [
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
            key0 === 'directory' ||
            key0 === 'length' ||
            key0 === 'path' ||
            key0 === 'sha256'
          )) {
            validate181.errors = [
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
          if (data.directory !== undefined) {
            const _errs2 = errors;
            if (typeof data.directory !== 'boolean') {
              validate181.errors = [
                {
                  instancePath: instancePath + '/directory',
                  schemaPath: '#/properties/directory/type',
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
            if (data.length !== undefined) {
              let data1 = data.length;
              const _errs4 = errors;
              if (!(typeof data1 == 'number' && !(data1 % 1) && !isNaN(data1) && isFinite(data1))) {
                validate181.errors = [
                  {
                    instancePath: instancePath + '/length',
                    schemaPath: '#/properties/length/type',
                    keyword: 'type',
                    params: { type: 'integer' },
                    message: 'must be integer',
                  },
                ];
                return false;
              }
              if (errors === _errs4) {
                if (typeof data1 == 'number' && isFinite(data1)) {
                  if (data1 > 9007199254740991 || isNaN(data1)) {
                    validate181.errors = [
                      {
                        instancePath: instancePath + '/length',
                        schemaPath: '#/properties/length/maximum',
                        keyword: 'maximum',
                        params: { comparison: '<=', limit: 9007199254740991 },
                        message: 'must be <= 9007199254740991',
                      },
                    ];
                    return false;
                  } else {
                    if (data1 < 0 || isNaN(data1)) {
                      validate181.errors = [
                        {
                          instancePath: instancePath + '/length',
                          schemaPath: '#/properties/length/minimum',
                          keyword: 'minimum',
                          params: { comparison: '>=', limit: 0 },
                          message: 'must be >= 0',
                        },
                      ];
                      return false;
                    }
                  }
                }
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.path !== undefined) {
                const _errs6 = errors;
                if (typeof data.path !== 'string') {
                  validate181.errors = [
                    {
                      instancePath: instancePath + '/path',
                      schemaPath: '#/properties/path/type',
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
                if (data.sha256 !== undefined) {
                  const _errs8 = errors;
                  if (typeof data.sha256 !== 'string') {
                    validate181.errors = [
                      {
                        instancePath: instancePath + '/sha256',
                        schemaPath: '#/properties/sha256/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      },
                    ];
                    return false;
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
      validate181.errors = [
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
  validate181.errors = vErrors;
  return errors === 0;
}
validate181.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema68 = {
  additionalProperties: false,
  properties: {
    bytes: { items: { maximum: 255, minimum: 0, type: 'integer' }, type: 'array' },
    eof: { type: 'boolean' },
    next_offset: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
    offset: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
  },
  required: ['offset', 'next_offset', 'bytes', 'eof'],
  type: 'object',
};
function validate183(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate183.evaluated;
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
        (data.offset === undefined && (missing0 = 'offset')) ||
        (data.next_offset === undefined && (missing0 = 'next_offset')) ||
        (data.bytes === undefined && (missing0 = 'bytes')) ||
        (data.eof === undefined && (missing0 = 'eof'))
      ) {
        validate183.errors = [
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
            key0 === 'bytes' ||
            key0 === 'eof' ||
            key0 === 'next_offset' ||
            key0 === 'offset'
          )) {
            validate183.errors = [
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
          if (data.bytes !== undefined) {
            let data0 = data.bytes;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  let data1 = data0[i0];
                  const _errs4 = errors;
                  if (!(
                    typeof data1 == 'number' &&
                    !(data1 % 1) &&
                    !isNaN(data1) &&
                    isFinite(data1)
                  )) {
                    validate183.errors = [
                      {
                        instancePath: instancePath + '/bytes/' + i0,
                        schemaPath: '#/properties/bytes/items/type',
                        keyword: 'type',
                        params: { type: 'integer' },
                        message: 'must be integer',
                      },
                    ];
                    return false;
                  }
                  if (errors === _errs4) {
                    if (typeof data1 == 'number' && isFinite(data1)) {
                      if (data1 > 255 || isNaN(data1)) {
                        validate183.errors = [
                          {
                            instancePath: instancePath + '/bytes/' + i0,
                            schemaPath: '#/properties/bytes/items/maximum',
                            keyword: 'maximum',
                            params: { comparison: '<=', limit: 255 },
                            message: 'must be <= 255',
                          },
                        ];
                        return false;
                      } else {
                        if (data1 < 0 || isNaN(data1)) {
                          validate183.errors = [
                            {
                              instancePath: instancePath + '/bytes/' + i0,
                              schemaPath: '#/properties/bytes/items/minimum',
                              keyword: 'minimum',
                              params: { comparison: '>=', limit: 0 },
                              message: 'must be >= 0',
                            },
                          ];
                          return false;
                        }
                      }
                    }
                  }
                  var valid1 = _errs4 === errors;
                  if (!valid1) {
                    break;
                  }
                }
              } else {
                validate183.errors = [
                  {
                    instancePath: instancePath + '/bytes',
                    schemaPath: '#/properties/bytes/type',
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
            if (data.eof !== undefined) {
              const _errs6 = errors;
              if (typeof data.eof !== 'boolean') {
                validate183.errors = [
                  {
                    instancePath: instancePath + '/eof',
                    schemaPath: '#/properties/eof/type',
                    keyword: 'type',
                    params: { type: 'boolean' },
                    message: 'must be boolean',
                  },
                ];
                return false;
              }
              var valid0 = _errs6 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.next_offset !== undefined) {
                let data3 = data.next_offset;
                const _errs8 = errors;
                if (!(
                  typeof data3 == 'number' &&
                  !(data3 % 1) &&
                  !isNaN(data3) &&
                  isFinite(data3)
                )) {
                  validate183.errors = [
                    {
                      instancePath: instancePath + '/next_offset',
                      schemaPath: '#/properties/next_offset/type',
                      keyword: 'type',
                      params: { type: 'integer' },
                      message: 'must be integer',
                    },
                  ];
                  return false;
                }
                if (errors === _errs8) {
                  if (typeof data3 == 'number' && isFinite(data3)) {
                    if (data3 > 9007199254740991 || isNaN(data3)) {
                      validate183.errors = [
                        {
                          instancePath: instancePath + '/next_offset',
                          schemaPath: '#/properties/next_offset/maximum',
                          keyword: 'maximum',
                          params: { comparison: '<=', limit: 9007199254740991 },
                          message: 'must be <= 9007199254740991',
                        },
                      ];
                      return false;
                    } else {
                      if (data3 < 0 || isNaN(data3)) {
                        validate183.errors = [
                          {
                            instancePath: instancePath + '/next_offset',
                            schemaPath: '#/properties/next_offset/minimum',
                            keyword: 'minimum',
                            params: { comparison: '>=', limit: 0 },
                            message: 'must be >= 0',
                          },
                        ];
                        return false;
                      }
                    }
                  }
                }
                var valid0 = _errs8 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.offset !== undefined) {
                  let data4 = data.offset;
                  const _errs10 = errors;
                  if (!(
                    typeof data4 == 'number' &&
                    !(data4 % 1) &&
                    !isNaN(data4) &&
                    isFinite(data4)
                  )) {
                    validate183.errors = [
                      {
                        instancePath: instancePath + '/offset',
                        schemaPath: '#/properties/offset/type',
                        keyword: 'type',
                        params: { type: 'integer' },
                        message: 'must be integer',
                      },
                    ];
                    return false;
                  }
                  if (errors === _errs10) {
                    if (typeof data4 == 'number' && isFinite(data4)) {
                      if (data4 > 9007199254740991 || isNaN(data4)) {
                        validate183.errors = [
                          {
                            instancePath: instancePath + '/offset',
                            schemaPath: '#/properties/offset/maximum',
                            keyword: 'maximum',
                            params: { comparison: '<=', limit: 9007199254740991 },
                            message: 'must be <= 9007199254740991',
                          },
                        ];
                        return false;
                      } else {
                        if (data4 < 0 || isNaN(data4)) {
                          validate183.errors = [
                            {
                              instancePath: instancePath + '/offset',
                              schemaPath: '#/properties/offset/minimum',
                              keyword: 'minimum',
                              params: { comparison: '>=', limit: 0 },
                              message: 'must be >= 0',
                            },
                          ];
                          return false;
                        }
                      }
                    }
                  }
                  var valid0 = _errs10 === errors;
                } else {
                  var valid0 = true;
                }
              }
            }
          }
        }
      }
    } else {
      validate183.errors = [
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
  validate183.errors = vErrors;
  return errors === 0;
}
validate183.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
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
            if ('memory.vault.timeline' !== data.command) {
              const err2 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/0/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.timeline' },
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
                !validate99(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate99.errors : vErrors.concat(validate99.errors);
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
            if ('memory.vault.read_many' !== data.command) {
              const err6 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/1/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.read_many' },
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
                !validate111(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate111.errors : vErrors.concat(validate111.errors);
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
            if ('memory.vaults.get' !== data.command) {
              const err10 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/2/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vaults.get' },
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
                !validate140(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate140.errors : vErrors.concat(validate140.errors);
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
            if ('memory.settings.get' !== data.command) {
              const err14 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/3/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.settings.get' },
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
                !validate157(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate157.errors : vErrors.concat(validate157.errors);
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
            if ('memory.settings.update' !== data.command) {
              const err18 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/4/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.settings.update' },
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
                !validate157(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate157.errors : vErrors.concat(validate157.errors);
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
            if ('memory.vault.validate' !== data.command) {
              const err22 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/5/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.validate' },
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
                !validate162(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate162.errors : vErrors.concat(validate162.errors);
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
            if ('memory.vault.rebuild' !== data.command) {
              const err26 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/6/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.rebuild' },
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
                !validate152(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate152.errors : vErrors.concat(validate152.errors);
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
  const _errs36 = errors;
  if (errors === _errs36) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing7;
      if (
        (data.command === undefined && (missing7 = 'command')) ||
        (data.value === undefined && (missing7 = 'value'))
      ) {
        const err28 = {
          instancePath,
          schemaPath: '#/anyOf/7/required',
          keyword: 'required',
          params: { missingProperty: missing7 },
          message: "must have required property '" + missing7 + "'",
        };
        if (vErrors === null) {
          vErrors = [err28];
        } else {
          vErrors.push(err28);
        }
        errors++;
      } else {
        const _errs38 = errors;
        for (const key7 in data) {
          if (!(key7 === 'command' || key7 === 'value')) {
            const err29 = {
              instancePath,
              schemaPath: '#/anyOf/7/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key7 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err29];
            } else {
              vErrors.push(err29);
            }
            errors++;
            break;
          }
        }
        if (_errs38 === errors) {
          if (data.command !== undefined) {
            const _errs39 = errors;
            if ('memory.vault.repair.preview' !== data.command) {
              const err30 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/7/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.repair.preview' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err30];
              } else {
                vErrors.push(err30);
              }
              errors++;
            }
            var valid8 = _errs39 === errors;
          } else {
            var valid8 = true;
          }
          if (valid8) {
            if (data.value !== undefined) {
              const _errs40 = errors;
              if (
                !validate167(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate167.errors : vErrors.concat(validate167.errors);
                errors = vErrors.length;
              }
              var valid8 = _errs40 === errors;
            } else {
              var valid8 = true;
            }
          }
        }
      }
    } else {
      const err31 = {
        instancePath,
        schemaPath: '#/anyOf/7/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err31];
      } else {
        vErrors.push(err31);
      }
      errors++;
    }
  }
  var _valid0 = _errs36 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs41 = errors;
  if (errors === _errs41) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing8;
      if (
        (data.command === undefined && (missing8 = 'command')) ||
        (data.value === undefined && (missing8 = 'value'))
      ) {
        const err32 = {
          instancePath,
          schemaPath: '#/anyOf/8/required',
          keyword: 'required',
          params: { missingProperty: missing8 },
          message: "must have required property '" + missing8 + "'",
        };
        if (vErrors === null) {
          vErrors = [err32];
        } else {
          vErrors.push(err32);
        }
        errors++;
      } else {
        const _errs43 = errors;
        for (const key8 in data) {
          if (!(key8 === 'command' || key8 === 'value')) {
            const err33 = {
              instancePath,
              schemaPath: '#/anyOf/8/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key8 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err33];
            } else {
              vErrors.push(err33);
            }
            errors++;
            break;
          }
        }
        if (_errs43 === errors) {
          if (data.command !== undefined) {
            const _errs44 = errors;
            if ('memory.vault.repair.apply' !== data.command) {
              const err34 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/8/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.repair.apply' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err34];
              } else {
                vErrors.push(err34);
              }
              errors++;
            }
            var valid9 = _errs44 === errors;
          } else {
            var valid9 = true;
          }
          if (valid9) {
            if (data.value !== undefined) {
              const _errs45 = errors;
              if (
                !validate172(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate172.errors : vErrors.concat(validate172.errors);
                errors = vErrors.length;
              }
              var valid9 = _errs45 === errors;
            } else {
              var valid9 = true;
            }
          }
        }
      }
    } else {
      const err35 = {
        instancePath,
        schemaPath: '#/anyOf/8/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err35];
      } else {
        vErrors.push(err35);
      }
      errors++;
    }
  }
  var _valid0 = _errs41 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs46 = errors;
  if (errors === _errs46) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing9;
      if (
        (data.command === undefined && (missing9 = 'command')) ||
        (data.value === undefined && (missing9 = 'value'))
      ) {
        const err36 = {
          instancePath,
          schemaPath: '#/anyOf/9/required',
          keyword: 'required',
          params: { missingProperty: missing9 },
          message: "must have required property '" + missing9 + "'",
        };
        if (vErrors === null) {
          vErrors = [err36];
        } else {
          vErrors.push(err36);
        }
        errors++;
      } else {
        const _errs48 = errors;
        for (const key9 in data) {
          if (!(key9 === 'command' || key9 === 'value')) {
            const err37 = {
              instancePath,
              schemaPath: '#/anyOf/9/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key9 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err37];
            } else {
              vErrors.push(err37);
            }
            errors++;
            break;
          }
        }
        if (_errs48 === errors) {
          if (data.command !== undefined) {
            const _errs49 = errors;
            if ('memory.vault.open_directory' !== data.command) {
              const err38 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/9/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.open_directory' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err38];
              } else {
                vErrors.push(err38);
              }
              errors++;
            }
            var valid10 = _errs49 === errors;
          } else {
            var valid10 = true;
          }
          if (valid10) {
            if (data.value !== undefined) {
              const _errs50 = errors;
              if (
                !validate175(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate175.errors : vErrors.concat(validate175.errors);
                errors = vErrors.length;
              }
              var valid10 = _errs50 === errors;
            } else {
              var valid10 = true;
            }
          }
        }
      }
    } else {
      const err39 = {
        instancePath,
        schemaPath: '#/anyOf/9/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err39];
      } else {
        vErrors.push(err39);
      }
      errors++;
    }
  }
  var _valid0 = _errs46 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs51 = errors;
  if (errors === _errs51) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing10;
      if (
        (data.command === undefined && (missing10 = 'command')) ||
        (data.value === undefined && (missing10 = 'value'))
      ) {
        const err40 = {
          instancePath,
          schemaPath: '#/anyOf/10/required',
          keyword: 'required',
          params: { missingProperty: missing10 },
          message: "must have required property '" + missing10 + "'",
        };
        if (vErrors === null) {
          vErrors = [err40];
        } else {
          vErrors.push(err40);
        }
        errors++;
      } else {
        const _errs53 = errors;
        for (const key10 in data) {
          if (!(key10 === 'command' || key10 === 'value')) {
            const err41 = {
              instancePath,
              schemaPath: '#/anyOf/10/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key10 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err41];
            } else {
              vErrors.push(err41);
            }
            errors++;
            break;
          }
        }
        if (_errs53 === errors) {
          if (data.command !== undefined) {
            const _errs54 = errors;
            if ('memory.maintenance.open_log' !== data.command) {
              const err42 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/10/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.maintenance.open_log' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err42];
              } else {
                vErrors.push(err42);
              }
              errors++;
            }
            var valid11 = _errs54 === errors;
          } else {
            var valid11 = true;
          }
          if (valid11) {
            if (data.value !== undefined) {
              const _errs55 = errors;
              if (
                !validate175(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate175.errors : vErrors.concat(validate175.errors);
                errors = vErrors.length;
              }
              var valid11 = _errs55 === errors;
            } else {
              var valid11 = true;
            }
          }
        }
      }
    } else {
      const err43 = {
        instancePath,
        schemaPath: '#/anyOf/10/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err43];
      } else {
        vErrors.push(err43);
      }
      errors++;
    }
  }
  var _valid0 = _errs51 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs56 = errors;
  if (errors === _errs56) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing11;
      if (
        (data.command === undefined && (missing11 = 'command')) ||
        (data.value === undefined && (missing11 = 'value'))
      ) {
        const err44 = {
          instancePath,
          schemaPath: '#/anyOf/11/required',
          keyword: 'required',
          params: { missingProperty: missing11 },
          message: "must have required property '" + missing11 + "'",
        };
        if (vErrors === null) {
          vErrors = [err44];
        } else {
          vErrors.push(err44);
        }
        errors++;
      } else {
        const _errs58 = errors;
        for (const key11 in data) {
          if (!(key11 === 'command' || key11 === 'value')) {
            const err45 = {
              instancePath,
              schemaPath: '#/anyOf/11/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key11 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err45];
            } else {
              vErrors.push(err45);
            }
            errors++;
            break;
          }
        }
        if (_errs58 === errors) {
          if (data.command !== undefined) {
            const _errs59 = errors;
            if ('memory.vault.export' !== data.command) {
              const err46 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/11/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.vault.export' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err46];
              } else {
                vErrors.push(err46);
              }
              errors++;
            }
            var valid12 = _errs59 === errors;
          } else {
            var valid12 = true;
          }
          if (valid12) {
            if (data.value !== undefined) {
              const _errs60 = errors;
              if (
                !validate175(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate175.errors : vErrors.concat(validate175.errors);
                errors = vErrors.length;
              }
              var valid12 = _errs60 === errors;
            } else {
              var valid12 = true;
            }
          }
        }
      }
    } else {
      const err47 = {
        instancePath,
        schemaPath: '#/anyOf/11/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err47];
      } else {
        vErrors.push(err47);
      }
      errors++;
    }
  }
  var _valid0 = _errs56 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs61 = errors;
  if (errors === _errs61) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing12;
      if (
        (data.command === undefined && (missing12 = 'command')) ||
        (data.value === undefined && (missing12 = 'value'))
      ) {
        const err48 = {
          instancePath,
          schemaPath: '#/anyOf/12/required',
          keyword: 'required',
          params: { missingProperty: missing12 },
          message: "must have required property '" + missing12 + "'",
        };
        if (vErrors === null) {
          vErrors = [err48];
        } else {
          vErrors.push(err48);
        }
        errors++;
      } else {
        const _errs63 = errors;
        for (const key12 in data) {
          if (!(key12 === 'command' || key12 === 'value')) {
            const err49 = {
              instancePath,
              schemaPath: '#/anyOf/12/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key12 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err49];
            } else {
              vErrors.push(err49);
            }
            errors++;
            break;
          }
        }
        if (_errs63 === errors) {
          if (data.command !== undefined) {
            const _errs64 = errors;
            if ('memory.export.prepare' !== data.command) {
              const err50 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/12/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.export.prepare' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err50];
              } else {
                vErrors.push(err50);
              }
              errors++;
            }
            var valid13 = _errs64 === errors;
          } else {
            var valid13 = true;
          }
          if (valid13) {
            if (data.value !== undefined) {
              const _errs65 = errors;
              if (
                !validate179(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate179.errors : vErrors.concat(validate179.errors);
                errors = vErrors.length;
              }
              var valid13 = _errs65 === errors;
            } else {
              var valid13 = true;
            }
          }
        }
      }
    } else {
      const err51 = {
        instancePath,
        schemaPath: '#/anyOf/12/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err51];
      } else {
        vErrors.push(err51);
      }
      errors++;
    }
  }
  var _valid0 = _errs61 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs66 = errors;
  if (errors === _errs66) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing13;
      if (
        (data.command === undefined && (missing13 = 'command')) ||
        (data.value === undefined && (missing13 = 'value'))
      ) {
        const err52 = {
          instancePath,
          schemaPath: '#/anyOf/13/required',
          keyword: 'required',
          params: { missingProperty: missing13 },
          message: "must have required property '" + missing13 + "'",
        };
        if (vErrors === null) {
          vErrors = [err52];
        } else {
          vErrors.push(err52);
        }
        errors++;
      } else {
        const _errs68 = errors;
        for (const key13 in data) {
          if (!(key13 === 'command' || key13 === 'value')) {
            const err53 = {
              instancePath,
              schemaPath: '#/anyOf/13/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key13 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err53];
            } else {
              vErrors.push(err53);
            }
            errors++;
            break;
          }
        }
        if (_errs68 === errors) {
          if (data.command !== undefined) {
            const _errs69 = errors;
            if ('memory.export.entry' !== data.command) {
              const err54 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/13/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.export.entry' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err54];
              } else {
                vErrors.push(err54);
              }
              errors++;
            }
            var valid14 = _errs69 === errors;
          } else {
            var valid14 = true;
          }
          if (valid14) {
            if (data.value !== undefined) {
              const _errs70 = errors;
              if (
                !validate181(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate181.errors : vErrors.concat(validate181.errors);
                errors = vErrors.length;
              }
              var valid14 = _errs70 === errors;
            } else {
              var valid14 = true;
            }
          }
        }
      }
    } else {
      const err55 = {
        instancePath,
        schemaPath: '#/anyOf/13/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err55];
      } else {
        vErrors.push(err55);
      }
      errors++;
    }
  }
  var _valid0 = _errs66 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs71 = errors;
  if (errors === _errs71) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing14;
      if (
        (data.command === undefined && (missing14 = 'command')) ||
        (data.value === undefined && (missing14 = 'value'))
      ) {
        const err56 = {
          instancePath,
          schemaPath: '#/anyOf/14/required',
          keyword: 'required',
          params: { missingProperty: missing14 },
          message: "must have required property '" + missing14 + "'",
        };
        if (vErrors === null) {
          vErrors = [err56];
        } else {
          vErrors.push(err56);
        }
        errors++;
      } else {
        const _errs73 = errors;
        for (const key14 in data) {
          if (!(key14 === 'command' || key14 === 'value')) {
            const err57 = {
              instancePath,
              schemaPath: '#/anyOf/14/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key14 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err57];
            } else {
              vErrors.push(err57);
            }
            errors++;
            break;
          }
        }
        if (_errs73 === errors) {
          if (data.command !== undefined) {
            const _errs74 = errors;
            if ('memory.export.chunk' !== data.command) {
              const err58 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/14/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.export.chunk' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err58];
              } else {
                vErrors.push(err58);
              }
              errors++;
            }
            var valid15 = _errs74 === errors;
          } else {
            var valid15 = true;
          }
          if (valid15) {
            if (data.value !== undefined) {
              const _errs75 = errors;
              if (
                !validate183(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate183.errors : vErrors.concat(validate183.errors);
                errors = vErrors.length;
              }
              var valid15 = _errs75 === errors;
            } else {
              var valid15 = true;
            }
          }
        }
      }
    } else {
      const err59 = {
        instancePath,
        schemaPath: '#/anyOf/14/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err59];
      } else {
        vErrors.push(err59);
      }
      errors++;
    }
  }
  var _valid0 = _errs71 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs76 = errors;
  if (errors === _errs76) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing15;
      if (
        (data.command === undefined && (missing15 = 'command')) ||
        (data.value === undefined && (missing15 = 'value'))
      ) {
        const err60 = {
          instancePath,
          schemaPath: '#/anyOf/15/required',
          keyword: 'required',
          params: { missingProperty: missing15 },
          message: "must have required property '" + missing15 + "'",
        };
        if (vErrors === null) {
          vErrors = [err60];
        } else {
          vErrors.push(err60);
        }
        errors++;
      } else {
        const _errs78 = errors;
        for (const key15 in data) {
          if (!(key15 === 'command' || key15 === 'value')) {
            const err61 = {
              instancePath,
              schemaPath: '#/anyOf/15/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key15 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err61];
            } else {
              vErrors.push(err61);
            }
            errors++;
            break;
          }
        }
        if (_errs78 === errors) {
          if (data.command !== undefined) {
            const _errs79 = errors;
            if ('memory.export.close' !== data.command) {
              const err62 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/15/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'memory.export.close' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err62];
              } else {
                vErrors.push(err62);
              }
              errors++;
            }
            var valid16 = _errs79 === errors;
          } else {
            var valid16 = true;
          }
          if (valid16) {
            if (data.value !== undefined) {
              const _errs80 = errors;
              if (
                !validate175(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate175.errors : vErrors.concat(validate175.errors);
                errors = vErrors.length;
              }
              var valid16 = _errs80 === errors;
            } else {
              var valid16 = true;
            }
          }
        }
      }
    } else {
      const err63 = {
        instancePath,
        schemaPath: '#/anyOf/15/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err63];
      } else {
        vErrors.push(err63);
      }
      errors++;
    }
  }
  var _valid0 = _errs76 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  if (!valid0) {
    const err64 = {
      instancePath,
      schemaPath: '#/anyOf',
      keyword: 'anyOf',
      params: {},
      message: 'must match a schema in anyOf',
    };
    if (vErrors === null) {
      vErrors = [err64];
    } else {
      vErrors.push(err64);
    }
    errors++;
    validate98.errors = vErrors;
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
  validate98.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate98.evaluated = { dynamicProps: true, dynamicItems: false };
