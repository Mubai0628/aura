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
export const validateRecoveryRequest = validate52;
const schema19 = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  anyOf: [
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'config.recovery.notice' },
        value: { type: 'object', additionalProperties: false },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'config.recovery.plan' },
        value: { $ref: '#/$defs/ConfigRecoveryPlanRequest' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'config.recovery.apply' },
        value: { $ref: '#/$defs/ConfigRecoveryApplyRequest' },
      },
    },
  ],
  $defs: {
    RecoveryNotice: {
      additionalProperties: false,
      properties: {
        affected_domains: {
          description: 'The configuration domains whose fault is being reported, by key.',
          items: { type: 'string' },
          type: 'array',
        },
        available_actions: {
          description: 'The actions the user may take, by key.',
          items: { type: 'string' },
          type: 'array',
        },
        fault_codes: {
          description: "Stable codes for each fault. What the user's report will quote.",
          items: { type: 'string' },
          type: 'array',
        },
        next_step: { description: 'A bounded next step for the user.', type: 'string' },
        recovery_in_progress: {
          description:
            'A recovery is still owned by this runtime; a state read is not its receipt.',
          type: 'boolean',
        },
        scope_known: {
          description:
            'Whether the runtime can say which domains are affected.\n\n`false` is the honest "unknown scope": the user is told the runtime cannot\nnarrow it, rather than being shown a misleadingly precise list.',
          type: 'boolean',
        },
        suspended_calls: {
          description: 'New calls that are suspended, described in bounded terms.',
          items: { type: 'string' },
          type: 'array',
        },
        unavailable_capabilities: {
          description: 'The capabilities that are unavailable because of those faults, by key.',
          items: { type: 'string' },
          type: 'array',
        },
      },
      required: [
        'affected_domains',
        'unavailable_capabilities',
        'fault_codes',
        'suspended_calls',
        'available_actions',
        'scope_known',
        'recovery_in_progress',
        'next_step',
      ],
      type: 'object',
    },
    ConfigRecoveryPlanRequest: {
      additionalProperties: false,
      properties: {
        domains: { default: [], items: { $ref: '#/$defs/ConfigModule' }, type: 'array' },
      },
      type: 'object',
    },
    ConfigModule: {
      enum: [
        'aura',
        'models',
        'tools',
        'hooks',
        'mcp',
        'skills',
        'commands',
        'plugins',
        'ui',
        'prompt',
        'security',
        'browser',
        'memory',
        'diagnostics',
        'update',
      ],
      type: 'string',
    },
    BundleRecoveryPlan: {
      additionalProperties: false,
      description:
        "The plan for a whole cross-domain recovery.\n\nA cross-domain restore must reach **one provable consistent version**,\nso the bundle plan is computed as a unit. It never staples each domain's own\nlast-known-good together and calls the result consistent: either a common\nrevision is provable, or the cross-domain restore is honestly disabled and the\nuser is offered a scoped default reset instead.",
      properties: {
        consistent_revision: {
          description: 'The common revision every affected domain can reach, when one exists.',
          minimum: 0,
          type: ['integer', 'null'],
          maximum: 9007199254740991,
        },
        consistent_set: {
          description:
            "The exact set the próven consistent version restores, when one exists.\n\nThis is the preview's own binding. `apply` consumes\n**this** list rather than re-selecting each domain's own latest history, so\nthe write cannot silently differ from what the user was shown. Each entry is\n`(module, revision, content digest of the target version)`.\n\nIt is empty exactly when no joint record proved a consistent set, which is\nalso when `consistent_revision` is `None`.",
          items: {
            maxItems: 3,
            minItems: 3,
            prefixItems: [
              { $ref: '#/$defs/ConfigModule' },
              { minimum: 0, type: 'integer', maximum: 9007199254740991 },
              { type: 'string' },
            ],
            type: 'array',
          },
          type: 'array',
        },
        domains: {
          description: 'Per-domain plans, so the user sees the whole scope before triggering.',
          items: { $ref: '#/$defs/DomainRecoveryPlan' },
          type: 'array',
        },
        inconsistency_code: {
          description: 'A bounded explanation when a consistent version cannot be proven.',
          type: ['string', 'null'],
        },
        modules: { items: { $ref: '#/$defs/ConfigModule' }, type: 'array' },
        options: { items: { $ref: '#/$defs/RecoveryOption' }, type: 'array' },
        preview_id: {
          description:
            'Core supplies this opaque binding; clients echo it without reimplementing hashing.',
          type: 'string',
        },
      },
      required: ['preview_id', 'modules', 'options', 'domains', 'consistent_set'],
      type: 'object',
    },
    DomainRecoveryPlan: {
      additionalProperties: false,
      description:
        "What one domain's recovery would do, before anything is written.\n\nThis is the **preview**. It names the affected domain, the options with their\nreal availability, the versions that could be restored, the references that\nwould need rebinding, and the actual loss.",
      properties: {
        current_revision: {
          description: 'The revision currently served, if the domain serves anything.',
          minimum: 0,
          type: ['integer', 'null'],
          maximum: 9007199254740991,
        },
        degraded: {
          description: "The domain's current state, as the health registry reports it.",
          type: 'boolean',
        },
        fault_code: {
          description: 'The stable classification of the fault that made it degraded, if any.',
          type: ['string', 'null'],
        },
        loss: {
          description:
            'What the user actually loses by restoring the previous version. Stated so\nthe choice is informed rather than a guess.',
          items: { type: 'string' },
          type: 'array',
        },
        module: { $ref: '#/$defs/ConfigModule' },
        next_revision: {
          description: 'The revision the recovery result would carry: always `current + 1`.',
          minimum: 0,
          type: 'integer',
          maximum: 9007199254740991,
        },
        options: { items: { $ref: '#/$defs/RecoveryOption' }, type: 'array' },
        preview_id: {
          description:
            'The identity of the state this preview was computed from.\n\nA confirmation must be bound to what\nthe user was actually shown. Measured defect: a preview of a rev1 reset was\nstill applied after a legitimate save to dark/rev2, so the stale\nconfirmation overwrote a newer, valid configuration — `current_revision` was\nnot part of any check at commit time.\n\nThe caller echoes this back and the write refuses when the domain has moved\nsince. It is derived from the real facts the preview was built from, so a\ncaller cannot forge a preview for a state it was never shown.',
          type: 'string',
        },
        recoverable_versions: {
          description: 'The recorded versions, newest first, with their current-validity verdict.',
          items: { $ref: '#/$defs/RecoverableVersion' },
          type: 'array',
        },
        references_needing_rebinding: {
          description:
            'Settings keys that reference something outside the domain and would need\nrebinding after a restore or reset. Names only; never their values.',
          items: { type: 'string' },
          type: 'array',
        },
        scope_known: {
          description: "Whether the runtime could establish this domain's recovery scope at all.",
          type: 'boolean',
        },
      },
      required: [
        'module',
        'degraded',
        'options',
        'recoverable_versions',
        'next_revision',
        'references_needing_rebinding',
        'loss',
        'scope_known',
        'preview_id',
      ],
      type: 'object',
    },
    RecoveryOption: {
      additionalProperties: false,
      description:
        'Whether one recovery option is available, and exactly why not when it is not.\n\nAn unavailable option is **stated**, never silently omitted and never replaced\nby a different action. That is what "honestly disable the option" means.',
      properties: {
        action: { $ref: '#/$defs/ConfigRecoveryAction' },
        available: { type: 'boolean' },
        detail: {
          description: 'A bounded, path-free explanation for the user.',
          type: ['string', 'null'],
        },
        reason_code: {
          description:
            'A stable code when `available` is false or when the option carries a\ncaveat. No path, no document body, nothing the user supplied.',
          type: ['string', 'null'],
        },
      },
      required: ['action', 'available'],
      type: 'object',
    },
    ConfigRecoveryAction: {
      description: 'Which recovery action the user is asking for.',
      oneOf: [
        {
          const: 'retry',
          description: "Try the domain's current file again, changing nothing on disk.",
          type: 'string',
        },
        {
          const: 'restore_previous_valid',
          description:
            'Re-apply the most recent recorded version that still passes current\nvalidation.',
          type: 'string',
        },
        {
          const: 'reset_to_default',
          description: "Write the domain's validated default.",
          type: 'string',
        },
      ],
    },
    RecoverableVersion: {
      additionalProperties: false,
      description: 'One recorded version the domain could be restored to.',
      properties: {
        passes_current_validation: {
          description:
            'Whether this version still passes the **current** validator and permission.',
          type: 'boolean',
        },
        rejection_code: {
          description: 'A stable code when it does not.',
          type: ['string', 'null'],
        },
        revision: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
      },
      required: ['revision', 'passes_current_validation'],
      type: 'object',
    },
    ConfigRecoveryApplyRequest: {
      additionalProperties: false,
      properties: {
        action: {
          $ref: '#/$defs/ConfigRecoveryAction',
          description: 'The action the user chose, from the previewed options.',
        },
        domain: {
          anyOf: [{ $ref: '#/$defs/ConfigModule' }, { type: 'null' }],
          default: null,
          description: 'A single domain, for a single-domain recovery.',
        },
        domains: {
          default: [],
          description: 'Several domains, for a cross-domain recovery that must reach one version.',
          items: { $ref: '#/$defs/ConfigModule' },
          type: 'array',
        },
        preview_id: {
          description:
            'The identity of the preview the user confirmed.\n\nThe apply used to **re-plan** from the\nrequest\'s domains and discard the preview entirely, so the plan the user was\nshown was never the plan applied, and a stale confirmation could overwrite\nnewer valid configuration. Carrying the previewed identity across the wire is\nwhat lets the write refuse a confirmation that no longer describes the live\nstate.\n\nIt is **required**. It used to be an `Option`, and\n`None` meant "apply the current state", which was a bypass: a caller that omitted the\nfield wrote configuration no user had confirmed. A missing identity is now refused\nbefore any write, exactly like a stale one.',
          type: 'string',
        },
      },
      required: ['action', 'preview_id'],
      type: 'object',
    },
    RecoveryReport: {
      additionalProperties: false,
      properties: {
        action: { $ref: '#/$defs/ConfigRecoveryAction' },
        outcomes: { items: { $ref: '#/$defs/RecoveryOutcome' }, type: 'array' },
        reason_code: {
          description: 'A stable code when the attempt did not fully apply.',
          type: ['string', 'null'],
        },
        restored_capabilities: {
          description: 'The capabilities whose service is **really present** after this attempt.',
          items: { type: 'string' },
          type: 'array',
        },
        unavailable_capabilities: {
          default: [],
          description:
            'The capabilities still unavailable afterwards, each with its stable reason.\n\nThe previous report had nowhere to put this, so a\ncapability whose construction genuinely failed was indistinguishable from one that came\nback. The user saw a cleared fault marker and concluded the feature worked.',
          items: { $ref: '#/$defs/UnavailableCapability' },
          type: 'array',
        },
      },
      required: ['action', 'outcomes', 'restored_capabilities'],
      type: 'object',
    },
    RecoveryOutcome: {
      additionalProperties: false,
      description: 'What one recovery action really did.',
      properties: {
        action: { $ref: '#/$defs/ConfigRecoveryAction' },
        applied: { type: 'boolean' },
        module: { $ref: '#/$defs/ConfigModule' },
        reason_code: {
          description: 'A stable code when the action did not apply.',
          type: ['string', 'null'],
        },
        revision: {
          description: 'The new revision the domain now serves, when the action applied.',
          minimum: 0,
          type: ['integer', 'null'],
          maximum: 9007199254740991,
        },
        source: {
          anyOf: [{ $ref: '#/$defs/ConfigSource' }, { type: 'null' }],
          description: 'The source the recovered snapshot came from.',
        },
      },
      required: ['module', 'action', 'applied'],
      type: 'object',
    },
    ConfigSource: {
      description:
        "Where a domain's currently-served snapshot came from.\n\nSerializable because a recovery outcome reports it to the user, and the\noutcome is part of a projection. The wire form is the stable snake_case key, so\na renderer can name the source without a second mapping table.",
      enum: ['default', 'file', 'last_known_good'],
      type: 'string',
    },
    UnavailableCapability: {
      additionalProperties: false,
      properties: {
        capability: { description: 'The capability, by stable key.', type: 'string' },
        reason: {
          description: 'Why it is still unavailable, in bounded, path-free terms.',
          type: 'string',
        },
      },
      required: ['capability', 'reason'],
      type: 'object',
    },
  },
};
const schema20 = {
  additionalProperties: false,
  properties: { domains: { default: [], items: { $ref: '#/$defs/ConfigModule' }, type: 'array' } },
  type: 'object',
};
const schema21 = {
  enum: [
    'aura',
    'models',
    'tools',
    'hooks',
    'mcp',
    'skills',
    'commands',
    'plugins',
    'ui',
    'prompt',
    'security',
    'browser',
    'memory',
    'diagnostics',
    'update',
  ],
  type: 'string',
};
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
  if (!(
    data === 'aura' ||
    data === 'models' ||
    data === 'tools' ||
    data === 'hooks' ||
    data === 'mcp' ||
    data === 'skills' ||
    data === 'commands' ||
    data === 'plugins' ||
    data === 'ui' ||
    data === 'prompt' ||
    data === 'security' ||
    data === 'browser' ||
    data === 'memory' ||
    data === 'diagnostics' ||
    data === 'update'
  )) {
    validate54.errors = [
      {
        instancePath,
        schemaPath: '#/enum',
        keyword: 'enum',
        params: { allowedValues: schema21.enum },
        message: 'must be equal to one of the allowed values',
      },
    ];
    return false;
  }
  validate54.errors = vErrors;
  return errors === 0;
}
validate54.evaluated = { dynamicProps: false, dynamicItems: false };
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
      const _errs1 = errors;
      for (const key0 in data) {
        if (!(key0 === 'domains')) {
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
        if (data.domains !== undefined) {
          let data0 = data.domains;
          const _errs2 = errors;
          if (errors === _errs2) {
            if (Array.isArray(data0)) {
              var valid1 = true;
              const len0 = data0.length;
              for (let i0 = 0; i0 < len0; i0++) {
                const _errs4 = errors;
                if (
                  !validate54(data0[i0], {
                    instancePath: instancePath + '/domains/' + i0,
                    parentData: data0,
                    parentDataProperty: i0,
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate54.errors : vErrors.concat(validate54.errors);
                  errors = vErrors.length;
                }
                var valid1 = _errs4 === errors;
                if (!valid1) {
                  break;
                }
              }
            } else {
              validate53.errors = [
                {
                  instancePath: instancePath + '/domains',
                  schemaPath: '#/properties/domains/type',
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
const schema22 = {
  additionalProperties: false,
  properties: {
    action: {
      $ref: '#/$defs/ConfigRecoveryAction',
      description: 'The action the user chose, from the previewed options.',
    },
    domain: {
      anyOf: [{ $ref: '#/$defs/ConfigModule' }, { type: 'null' }],
      default: null,
      description: 'A single domain, for a single-domain recovery.',
    },
    domains: {
      default: [],
      description: 'Several domains, for a cross-domain recovery that must reach one version.',
      items: { $ref: '#/$defs/ConfigModule' },
      type: 'array',
    },
    preview_id: {
      description:
        'The identity of the preview the user confirmed.\n\nThe apply used to **re-plan** from the\nrequest\'s domains and discard the preview entirely, so the plan the user was\nshown was never the plan applied, and a stale confirmation could overwrite\nnewer valid configuration. Carrying the previewed identity across the wire is\nwhat lets the write refuse a confirmation that no longer describes the live\nstate.\n\nIt is **required**. It used to be an `Option`, and\n`None` meant "apply the current state", which was a bypass: a caller that omitted the\nfield wrote configuration no user had confirmed. A missing identity is now refused\nbefore any write, exactly like a stale one.',
      type: 'string',
    },
  },
  required: ['action', 'preview_id'],
  type: 'object',
};
const schema23 = {
  description: 'Which recovery action the user is asking for.',
  oneOf: [
    {
      const: 'retry',
      description: "Try the domain's current file again, changing nothing on disk.",
      type: 'string',
    },
    {
      const: 'restore_previous_valid',
      description:
        'Re-apply the most recent recorded version that still passes current\nvalidation.',
      type: 'string',
    },
    {
      const: 'reset_to_default',
      description: "Write the domain's validated default.",
      type: 'string',
    },
  ],
};
function validate58(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate58.evaluated;
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
  if (typeof data !== 'string') {
    const err0 = {
      instancePath,
      schemaPath: '#/oneOf/0/type',
      keyword: 'type',
      params: { type: 'string' },
      message: 'must be string',
    };
    if (vErrors === null) {
      vErrors = [err0];
    } else {
      vErrors.push(err0);
    }
    errors++;
  }
  if ('retry' !== data) {
    const err1 = {
      instancePath,
      schemaPath: '#/oneOf/0/const',
      keyword: 'const',
      params: { allowedValue: 'retry' },
      message: 'must be equal to constant',
    };
    if (vErrors === null) {
      vErrors = [err1];
    } else {
      vErrors.push(err1);
    }
    errors++;
  }
  var _valid0 = _errs1 === errors;
  if (_valid0) {
    valid0 = true;
    passing0 = 0;
  }
  const _errs3 = errors;
  if (typeof data !== 'string') {
    const err2 = {
      instancePath,
      schemaPath: '#/oneOf/1/type',
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
  if ('restore_previous_valid' !== data) {
    const err3 = {
      instancePath,
      schemaPath: '#/oneOf/1/const',
      keyword: 'const',
      params: { allowedValue: 'restore_previous_valid' },
      message: 'must be equal to constant',
    };
    if (vErrors === null) {
      vErrors = [err3];
    } else {
      vErrors.push(err3);
    }
    errors++;
  }
  var _valid0 = _errs3 === errors;
  if (_valid0 && valid0) {
    valid0 = false;
    passing0 = [passing0, 1];
  } else {
    if (_valid0) {
      valid0 = true;
      passing0 = 1;
    }
    const _errs5 = errors;
    if (typeof data !== 'string') {
      const err4 = {
        instancePath,
        schemaPath: '#/oneOf/2/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      };
      if (vErrors === null) {
        vErrors = [err4];
      } else {
        vErrors.push(err4);
      }
      errors++;
    }
    if ('reset_to_default' !== data) {
      const err5 = {
        instancePath,
        schemaPath: '#/oneOf/2/const',
        keyword: 'const',
        params: { allowedValue: 'reset_to_default' },
        message: 'must be equal to constant',
      };
      if (vErrors === null) {
        vErrors = [err5];
      } else {
        vErrors.push(err5);
      }
      errors++;
    }
    var _valid0 = _errs5 === errors;
    if (_valid0 && valid0) {
      valid0 = false;
      passing0 = [passing0, 2];
    } else {
      if (_valid0) {
        valid0 = true;
        passing0 = 2;
      }
    }
  }
  if (!valid0) {
    const err6 = {
      instancePath,
      schemaPath: '#/oneOf',
      keyword: 'oneOf',
      params: { passingSchemas: passing0 },
      message: 'must match exactly one schema in oneOf',
    };
    if (vErrors === null) {
      vErrors = [err6];
    } else {
      vErrors.push(err6);
    }
    errors++;
    validate58.errors = vErrors;
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
  validate58.errors = vErrors;
  return errors === 0;
}
validate58.evaluated = { dynamicProps: false, dynamicItems: false };
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
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.action === undefined && (missing0 = 'action')) ||
        (data.preview_id === undefined && (missing0 = 'preview_id'))
      ) {
        validate57.errors = [
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
            key0 === 'action' ||
            key0 === 'domain' ||
            key0 === 'domains' ||
            key0 === 'preview_id'
          )) {
            validate57.errors = [
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
            if (
              !validate58(data.action, {
                instancePath: instancePath + '/action',
                parentData: data,
                parentDataProperty: 'action',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate58.errors : vErrors.concat(validate58.errors);
              errors = vErrors.length;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.domain !== undefined) {
              let data1 = data.domain;
              const _errs3 = errors;
              const _errs4 = errors;
              let valid1 = false;
              const _errs5 = errors;
              if (
                !validate54(data1, {
                  instancePath: instancePath + '/domain',
                  parentData: data,
                  parentDataProperty: 'domain',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate54.errors : vErrors.concat(validate54.errors);
                errors = vErrors.length;
              }
              var _valid0 = _errs5 === errors;
              valid1 = valid1 || _valid0;
              const _errs6 = errors;
              if (data1 !== null) {
                const err0 = {
                  instancePath: instancePath + '/domain',
                  schemaPath: '#/properties/domain/anyOf/1/type',
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
                  instancePath: instancePath + '/domain',
                  schemaPath: '#/properties/domain/anyOf',
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
                validate57.errors = vErrors;
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
              if (data.domains !== undefined) {
                let data2 = data.domains;
                const _errs8 = errors;
                if (errors === _errs8) {
                  if (Array.isArray(data2)) {
                    var valid2 = true;
                    const len0 = data2.length;
                    for (let i0 = 0; i0 < len0; i0++) {
                      const _errs10 = errors;
                      if (
                        !validate54(data2[i0], {
                          instancePath: instancePath + '/domains/' + i0,
                          parentData: data2,
                          parentDataProperty: i0,
                          rootData,
                          dynamicAnchors,
                        })
                      ) {
                        vErrors =
                          vErrors === null ? validate54.errors : vErrors.concat(validate54.errors);
                        errors = vErrors.length;
                      }
                      var valid2 = _errs10 === errors;
                      if (!valid2) {
                        break;
                      }
                    }
                  } else {
                    validate57.errors = [
                      {
                        instancePath: instancePath + '/domains',
                        schemaPath: '#/properties/domains/type',
                        keyword: 'type',
                        params: { type: 'array' },
                        message: 'must be array',
                      },
                    ];
                    return false;
                  }
                }
                var valid0 = _errs8 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.preview_id !== undefined) {
                  const _errs11 = errors;
                  if (typeof data.preview_id !== 'string') {
                    validate57.errors = [
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
                  var valid0 = _errs11 === errors;
                } else {
                  var valid0 = true;
                }
              }
            }
          }
        }
      }
    } else {
      validate57.errors = [
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
  validate57.errors = vErrors;
  return errors === 0;
}
validate57.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
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
            if ('config.recovery.notice' !== data.command) {
              const err2 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/0/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'config.recovery.notice' },
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
              let data1 = data.value;
              const _errs5 = errors;
              if (errors === _errs5) {
                if (data1 && typeof data1 == 'object' && !Array.isArray(data1)) {
                  for (const key1 in data1) {
                    const err3 = {
                      instancePath: instancePath + '/value',
                      schemaPath: '#/anyOf/0/properties/value/additionalProperties',
                      keyword: 'additionalProperties',
                      params: { additionalProperty: key1 },
                      message: 'must NOT have additional properties',
                    };
                    if (vErrors === null) {
                      vErrors = [err3];
                    } else {
                      vErrors.push(err3);
                    }
                    errors++;
                    break;
                  }
                } else {
                  const err4 = {
                    instancePath: instancePath + '/value',
                    schemaPath: '#/anyOf/0/properties/value/type',
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
              var valid1 = _errs5 === errors;
            } else {
              var valid1 = true;
            }
          }
        }
      }
    } else {
      const err5 = {
        instancePath,
        schemaPath: '#/anyOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err5];
      } else {
        vErrors.push(err5);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    var props0 = true;
  }
  const _errs8 = errors;
  if (errors === _errs8) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (
        (data.command === undefined && (missing1 = 'command')) ||
        (data.value === undefined && (missing1 = 'value'))
      ) {
        const err6 = {
          instancePath,
          schemaPath: '#/anyOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err6];
        } else {
          vErrors.push(err6);
        }
        errors++;
      } else {
        const _errs10 = errors;
        for (const key2 in data) {
          if (!(key2 === 'command' || key2 === 'value')) {
            const err7 = {
              instancePath,
              schemaPath: '#/anyOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key2 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err7];
            } else {
              vErrors.push(err7);
            }
            errors++;
            break;
          }
        }
        if (_errs10 === errors) {
          if (data.command !== undefined) {
            const _errs11 = errors;
            if ('config.recovery.plan' !== data.command) {
              const err8 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/1/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'config.recovery.plan' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err8];
              } else {
                vErrors.push(err8);
              }
              errors++;
            }
            var valid2 = _errs11 === errors;
          } else {
            var valid2 = true;
          }
          if (valid2) {
            if (data.value !== undefined) {
              const _errs12 = errors;
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
              var valid2 = _errs12 === errors;
            } else {
              var valid2 = true;
            }
          }
        }
      }
    } else {
      const err9 = {
        instancePath,
        schemaPath: '#/anyOf/1/type',
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
  var _valid0 = _errs8 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs13 = errors;
  if (errors === _errs13) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing2;
      if (
        (data.command === undefined && (missing2 = 'command')) ||
        (data.value === undefined && (missing2 = 'value'))
      ) {
        const err10 = {
          instancePath,
          schemaPath: '#/anyOf/2/required',
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
        const _errs15 = errors;
        for (const key3 in data) {
          if (!(key3 === 'command' || key3 === 'value')) {
            const err11 = {
              instancePath,
              schemaPath: '#/anyOf/2/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key3 },
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
        if (_errs15 === errors) {
          if (data.command !== undefined) {
            const _errs16 = errors;
            if ('config.recovery.apply' !== data.command) {
              const err12 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/2/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'config.recovery.apply' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err12];
              } else {
                vErrors.push(err12);
              }
              errors++;
            }
            var valid3 = _errs16 === errors;
          } else {
            var valid3 = true;
          }
          if (valid3) {
            if (data.value !== undefined) {
              const _errs17 = errors;
              if (
                !validate57(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate57.errors : vErrors.concat(validate57.errors);
                errors = vErrors.length;
              }
              var valid3 = _errs17 === errors;
            } else {
              var valid3 = true;
            }
          }
        }
      }
    } else {
      const err13 = {
        instancePath,
        schemaPath: '#/anyOf/2/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err13];
      } else {
        vErrors.push(err13);
      }
      errors++;
    }
  }
  var _valid0 = _errs13 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  if (!valid0) {
    const err14 = {
      instancePath,
      schemaPath: '#/anyOf',
      keyword: 'anyOf',
      params: {},
      message: 'must match a schema in anyOf',
    };
    if (vErrors === null) {
      vErrors = [err14];
    } else {
      vErrors.push(err14);
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
export const validateRecoveryResponse = validate63;
const schema24 = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  anyOf: [
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'config.recovery.notice' },
        value: { $ref: '#/$defs/RecoveryNotice' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'config.recovery.plan' },
        value: { $ref: '#/$defs/BundleRecoveryPlan' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'config.recovery.apply' },
        value: { $ref: '#/$defs/RecoveryReport' },
      },
    },
  ],
  $defs: {
    RecoveryNotice: {
      additionalProperties: false,
      properties: {
        affected_domains: {
          description: 'The configuration domains whose fault is being reported, by key.',
          items: { type: 'string' },
          type: 'array',
        },
        available_actions: {
          description: 'The actions the user may take, by key.',
          items: { type: 'string' },
          type: 'array',
        },
        fault_codes: {
          description: "Stable codes for each fault. What the user's report will quote.",
          items: { type: 'string' },
          type: 'array',
        },
        next_step: { description: 'A bounded next step for the user.', type: 'string' },
        recovery_in_progress: {
          description:
            'A recovery is still owned by this runtime; a state read is not its receipt.',
          type: 'boolean',
        },
        scope_known: {
          description:
            'Whether the runtime can say which domains are affected.\n\n`false` is the honest "unknown scope": the user is told the runtime cannot\nnarrow it, rather than being shown a misleadingly precise list.',
          type: 'boolean',
        },
        suspended_calls: {
          description: 'New calls that are suspended, described in bounded terms.',
          items: { type: 'string' },
          type: 'array',
        },
        unavailable_capabilities: {
          description: 'The capabilities that are unavailable because of those faults, by key.',
          items: { type: 'string' },
          type: 'array',
        },
      },
      required: [
        'affected_domains',
        'unavailable_capabilities',
        'fault_codes',
        'suspended_calls',
        'available_actions',
        'scope_known',
        'recovery_in_progress',
        'next_step',
      ],
      type: 'object',
    },
    ConfigRecoveryPlanRequest: {
      additionalProperties: false,
      properties: {
        domains: { default: [], items: { $ref: '#/$defs/ConfigModule' }, type: 'array' },
      },
      type: 'object',
    },
    ConfigModule: {
      enum: [
        'aura',
        'models',
        'tools',
        'hooks',
        'mcp',
        'skills',
        'commands',
        'plugins',
        'ui',
        'prompt',
        'security',
        'browser',
        'memory',
        'diagnostics',
        'update',
      ],
      type: 'string',
    },
    BundleRecoveryPlan: {
      additionalProperties: false,
      description:
        "The plan for a whole cross-domain recovery.\n\nA cross-domain restore must reach **one provable consistent version**,\nso the bundle plan is computed as a unit. It never staples each domain's own\nlast-known-good together and calls the result consistent: either a common\nrevision is provable, or the cross-domain restore is honestly disabled and the\nuser is offered a scoped default reset instead.",
      properties: {
        consistent_revision: {
          description: 'The common revision every affected domain can reach, when one exists.',
          minimum: 0,
          type: ['integer', 'null'],
          maximum: 9007199254740991,
        },
        consistent_set: {
          description:
            "The exact set the próven consistent version restores, when one exists.\n\nThis is the preview's own binding. `apply` consumes\n**this** list rather than re-selecting each domain's own latest history, so\nthe write cannot silently differ from what the user was shown. Each entry is\n`(module, revision, content digest of the target version)`.\n\nIt is empty exactly when no joint record proved a consistent set, which is\nalso when `consistent_revision` is `None`.",
          items: {
            maxItems: 3,
            minItems: 3,
            prefixItems: [
              { $ref: '#/$defs/ConfigModule' },
              { minimum: 0, type: 'integer', maximum: 9007199254740991 },
              { type: 'string' },
            ],
            type: 'array',
          },
          type: 'array',
        },
        domains: {
          description: 'Per-domain plans, so the user sees the whole scope before triggering.',
          items: { $ref: '#/$defs/DomainRecoveryPlan' },
          type: 'array',
        },
        inconsistency_code: {
          description: 'A bounded explanation when a consistent version cannot be proven.',
          type: ['string', 'null'],
        },
        modules: { items: { $ref: '#/$defs/ConfigModule' }, type: 'array' },
        options: { items: { $ref: '#/$defs/RecoveryOption' }, type: 'array' },
        preview_id: {
          description:
            'Core supplies this opaque binding; clients echo it without reimplementing hashing.',
          type: 'string',
        },
      },
      required: ['preview_id', 'modules', 'options', 'domains', 'consistent_set'],
      type: 'object',
    },
    DomainRecoveryPlan: {
      additionalProperties: false,
      description:
        "What one domain's recovery would do, before anything is written.\n\nThis is the **preview**. It names the affected domain, the options with their\nreal availability, the versions that could be restored, the references that\nwould need rebinding, and the actual loss.",
      properties: {
        current_revision: {
          description: 'The revision currently served, if the domain serves anything.',
          minimum: 0,
          type: ['integer', 'null'],
          maximum: 9007199254740991,
        },
        degraded: {
          description: "The domain's current state, as the health registry reports it.",
          type: 'boolean',
        },
        fault_code: {
          description: 'The stable classification of the fault that made it degraded, if any.',
          type: ['string', 'null'],
        },
        loss: {
          description:
            'What the user actually loses by restoring the previous version. Stated so\nthe choice is informed rather than a guess.',
          items: { type: 'string' },
          type: 'array',
        },
        module: { $ref: '#/$defs/ConfigModule' },
        next_revision: {
          description: 'The revision the recovery result would carry: always `current + 1`.',
          minimum: 0,
          type: 'integer',
          maximum: 9007199254740991,
        },
        options: { items: { $ref: '#/$defs/RecoveryOption' }, type: 'array' },
        preview_id: {
          description:
            'The identity of the state this preview was computed from.\n\nA confirmation must be bound to what\nthe user was actually shown. Measured defect: a preview of a rev1 reset was\nstill applied after a legitimate save to dark/rev2, so the stale\nconfirmation overwrote a newer, valid configuration — `current_revision` was\nnot part of any check at commit time.\n\nThe caller echoes this back and the write refuses when the domain has moved\nsince. It is derived from the real facts the preview was built from, so a\ncaller cannot forge a preview for a state it was never shown.',
          type: 'string',
        },
        recoverable_versions: {
          description: 'The recorded versions, newest first, with their current-validity verdict.',
          items: { $ref: '#/$defs/RecoverableVersion' },
          type: 'array',
        },
        references_needing_rebinding: {
          description:
            'Settings keys that reference something outside the domain and would need\nrebinding after a restore or reset. Names only; never their values.',
          items: { type: 'string' },
          type: 'array',
        },
        scope_known: {
          description: "Whether the runtime could establish this domain's recovery scope at all.",
          type: 'boolean',
        },
      },
      required: [
        'module',
        'degraded',
        'options',
        'recoverable_versions',
        'next_revision',
        'references_needing_rebinding',
        'loss',
        'scope_known',
        'preview_id',
      ],
      type: 'object',
    },
    RecoveryOption: {
      additionalProperties: false,
      description:
        'Whether one recovery option is available, and exactly why not when it is not.\n\nAn unavailable option is **stated**, never silently omitted and never replaced\nby a different action. That is what "honestly disable the option" means.',
      properties: {
        action: { $ref: '#/$defs/ConfigRecoveryAction' },
        available: { type: 'boolean' },
        detail: {
          description: 'A bounded, path-free explanation for the user.',
          type: ['string', 'null'],
        },
        reason_code: {
          description:
            'A stable code when `available` is false or when the option carries a\ncaveat. No path, no document body, nothing the user supplied.',
          type: ['string', 'null'],
        },
      },
      required: ['action', 'available'],
      type: 'object',
    },
    ConfigRecoveryAction: {
      description: 'Which recovery action the user is asking for.',
      oneOf: [
        {
          const: 'retry',
          description: "Try the domain's current file again, changing nothing on disk.",
          type: 'string',
        },
        {
          const: 'restore_previous_valid',
          description:
            'Re-apply the most recent recorded version that still passes current\nvalidation.',
          type: 'string',
        },
        {
          const: 'reset_to_default',
          description: "Write the domain's validated default.",
          type: 'string',
        },
      ],
    },
    RecoverableVersion: {
      additionalProperties: false,
      description: 'One recorded version the domain could be restored to.',
      properties: {
        passes_current_validation: {
          description:
            'Whether this version still passes the **current** validator and permission.',
          type: 'boolean',
        },
        rejection_code: {
          description: 'A stable code when it does not.',
          type: ['string', 'null'],
        },
        revision: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
      },
      required: ['revision', 'passes_current_validation'],
      type: 'object',
    },
    ConfigRecoveryApplyRequest: {
      additionalProperties: false,
      properties: {
        action: {
          $ref: '#/$defs/ConfigRecoveryAction',
          description: 'The action the user chose, from the previewed options.',
        },
        domain: {
          anyOf: [{ $ref: '#/$defs/ConfigModule' }, { type: 'null' }],
          default: null,
          description: 'A single domain, for a single-domain recovery.',
        },
        domains: {
          default: [],
          description: 'Several domains, for a cross-domain recovery that must reach one version.',
          items: { $ref: '#/$defs/ConfigModule' },
          type: 'array',
        },
        preview_id: {
          description:
            'The identity of the preview the user confirmed.\n\nThe apply used to **re-plan** from the\nrequest\'s domains and discard the preview entirely, so the plan the user was\nshown was never the plan applied, and a stale confirmation could overwrite\nnewer valid configuration. Carrying the previewed identity across the wire is\nwhat lets the write refuse a confirmation that no longer describes the live\nstate.\n\nIt is **required**. It used to be an `Option`, and\n`None` meant "apply the current state", which was a bypass: a caller that omitted the\nfield wrote configuration no user had confirmed. A missing identity is now refused\nbefore any write, exactly like a stale one.',
          type: 'string',
        },
      },
      required: ['action', 'preview_id'],
      type: 'object',
    },
    RecoveryReport: {
      additionalProperties: false,
      properties: {
        action: { $ref: '#/$defs/ConfigRecoveryAction' },
        outcomes: { items: { $ref: '#/$defs/RecoveryOutcome' }, type: 'array' },
        reason_code: {
          description: 'A stable code when the attempt did not fully apply.',
          type: ['string', 'null'],
        },
        restored_capabilities: {
          description: 'The capabilities whose service is **really present** after this attempt.',
          items: { type: 'string' },
          type: 'array',
        },
        unavailable_capabilities: {
          default: [],
          description:
            'The capabilities still unavailable afterwards, each with its stable reason.\n\nThe previous report had nowhere to put this, so a\ncapability whose construction genuinely failed was indistinguishable from one that came\nback. The user saw a cleared fault marker and concluded the feature worked.',
          items: { $ref: '#/$defs/UnavailableCapability' },
          type: 'array',
        },
      },
      required: ['action', 'outcomes', 'restored_capabilities'],
      type: 'object',
    },
    RecoveryOutcome: {
      additionalProperties: false,
      description: 'What one recovery action really did.',
      properties: {
        action: { $ref: '#/$defs/ConfigRecoveryAction' },
        applied: { type: 'boolean' },
        module: { $ref: '#/$defs/ConfigModule' },
        reason_code: {
          description: 'A stable code when the action did not apply.',
          type: ['string', 'null'],
        },
        revision: {
          description: 'The new revision the domain now serves, when the action applied.',
          minimum: 0,
          type: ['integer', 'null'],
          maximum: 9007199254740991,
        },
        source: {
          anyOf: [{ $ref: '#/$defs/ConfigSource' }, { type: 'null' }],
          description: 'The source the recovered snapshot came from.',
        },
      },
      required: ['module', 'action', 'applied'],
      type: 'object',
    },
    ConfigSource: {
      description:
        "Where a domain's currently-served snapshot came from.\n\nSerializable because a recovery outcome reports it to the user, and the\noutcome is part of a projection. The wire form is the stable snake_case key, so\na renderer can name the source without a second mapping table.",
      enum: ['default', 'file', 'last_known_good'],
      type: 'string',
    },
    UnavailableCapability: {
      additionalProperties: false,
      properties: {
        capability: { description: 'The capability, by stable key.', type: 'string' },
        reason: {
          description: 'Why it is still unavailable, in bounded, path-free terms.',
          type: 'string',
        },
      },
      required: ['capability', 'reason'],
      type: 'object',
    },
  },
};
const schema25 = {
  additionalProperties: false,
  properties: {
    affected_domains: {
      description: 'The configuration domains whose fault is being reported, by key.',
      items: { type: 'string' },
      type: 'array',
    },
    available_actions: {
      description: 'The actions the user may take, by key.',
      items: { type: 'string' },
      type: 'array',
    },
    fault_codes: {
      description: "Stable codes for each fault. What the user's report will quote.",
      items: { type: 'string' },
      type: 'array',
    },
    next_step: { description: 'A bounded next step for the user.', type: 'string' },
    recovery_in_progress: {
      description: 'A recovery is still owned by this runtime; a state read is not its receipt.',
      type: 'boolean',
    },
    scope_known: {
      description:
        'Whether the runtime can say which domains are affected.\n\n`false` is the honest "unknown scope": the user is told the runtime cannot\nnarrow it, rather than being shown a misleadingly precise list.',
      type: 'boolean',
    },
    suspended_calls: {
      description: 'New calls that are suspended, described in bounded terms.',
      items: { type: 'string' },
      type: 'array',
    },
    unavailable_capabilities: {
      description: 'The capabilities that are unavailable because of those faults, by key.',
      items: { type: 'string' },
      type: 'array',
    },
  },
  required: [
    'affected_domains',
    'unavailable_capabilities',
    'fault_codes',
    'suspended_calls',
    'available_actions',
    'scope_known',
    'recovery_in_progress',
    'next_step',
  ],
  type: 'object',
};
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
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.affected_domains === undefined && (missing0 = 'affected_domains')) ||
        (data.unavailable_capabilities === undefined && (missing0 = 'unavailable_capabilities')) ||
        (data.fault_codes === undefined && (missing0 = 'fault_codes')) ||
        (data.suspended_calls === undefined && (missing0 = 'suspended_calls')) ||
        (data.available_actions === undefined && (missing0 = 'available_actions')) ||
        (data.scope_known === undefined && (missing0 = 'scope_known')) ||
        (data.recovery_in_progress === undefined && (missing0 = 'recovery_in_progress')) ||
        (data.next_step === undefined && (missing0 = 'next_step'))
      ) {
        validate64.errors = [
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
            key0 === 'affected_domains' ||
            key0 === 'available_actions' ||
            key0 === 'fault_codes' ||
            key0 === 'next_step' ||
            key0 === 'recovery_in_progress' ||
            key0 === 'scope_known' ||
            key0 === 'suspended_calls' ||
            key0 === 'unavailable_capabilities'
          )) {
            validate64.errors = [
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
          if (data.affected_domains !== undefined) {
            let data0 = data.affected_domains;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  const _errs4 = errors;
                  if (typeof data0[i0] !== 'string') {
                    validate64.errors = [
                      {
                        instancePath: instancePath + '/affected_domains/' + i0,
                        schemaPath: '#/properties/affected_domains/items/type',
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
                validate64.errors = [
                  {
                    instancePath: instancePath + '/affected_domains',
                    schemaPath: '#/properties/affected_domains/type',
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
            if (data.available_actions !== undefined) {
              let data2 = data.available_actions;
              const _errs6 = errors;
              if (errors === _errs6) {
                if (Array.isArray(data2)) {
                  var valid2 = true;
                  const len1 = data2.length;
                  for (let i1 = 0; i1 < len1; i1++) {
                    const _errs8 = errors;
                    if (typeof data2[i1] !== 'string') {
                      validate64.errors = [
                        {
                          instancePath: instancePath + '/available_actions/' + i1,
                          schemaPath: '#/properties/available_actions/items/type',
                          keyword: 'type',
                          params: { type: 'string' },
                          message: 'must be string',
                        },
                      ];
                      return false;
                    }
                    var valid2 = _errs8 === errors;
                    if (!valid2) {
                      break;
                    }
                  }
                } else {
                  validate64.errors = [
                    {
                      instancePath: instancePath + '/available_actions',
                      schemaPath: '#/properties/available_actions/type',
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
              if (data.fault_codes !== undefined) {
                let data4 = data.fault_codes;
                const _errs10 = errors;
                if (errors === _errs10) {
                  if (Array.isArray(data4)) {
                    var valid3 = true;
                    const len2 = data4.length;
                    for (let i2 = 0; i2 < len2; i2++) {
                      const _errs12 = errors;
                      if (typeof data4[i2] !== 'string') {
                        validate64.errors = [
                          {
                            instancePath: instancePath + '/fault_codes/' + i2,
                            schemaPath: '#/properties/fault_codes/items/type',
                            keyword: 'type',
                            params: { type: 'string' },
                            message: 'must be string',
                          },
                        ];
                        return false;
                      }
                      var valid3 = _errs12 === errors;
                      if (!valid3) {
                        break;
                      }
                    }
                  } else {
                    validate64.errors = [
                      {
                        instancePath: instancePath + '/fault_codes',
                        schemaPath: '#/properties/fault_codes/type',
                        keyword: 'type',
                        params: { type: 'array' },
                        message: 'must be array',
                      },
                    ];
                    return false;
                  }
                }
                var valid0 = _errs10 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.next_step !== undefined) {
                  const _errs14 = errors;
                  if (typeof data.next_step !== 'string') {
                    validate64.errors = [
                      {
                        instancePath: instancePath + '/next_step',
                        schemaPath: '#/properties/next_step/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs14 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.recovery_in_progress !== undefined) {
                    const _errs16 = errors;
                    if (typeof data.recovery_in_progress !== 'boolean') {
                      validate64.errors = [
                        {
                          instancePath: instancePath + '/recovery_in_progress',
                          schemaPath: '#/properties/recovery_in_progress/type',
                          keyword: 'type',
                          params: { type: 'boolean' },
                          message: 'must be boolean',
                        },
                      ];
                      return false;
                    }
                    var valid0 = _errs16 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.scope_known !== undefined) {
                      const _errs18 = errors;
                      if (typeof data.scope_known !== 'boolean') {
                        validate64.errors = [
                          {
                            instancePath: instancePath + '/scope_known',
                            schemaPath: '#/properties/scope_known/type',
                            keyword: 'type',
                            params: { type: 'boolean' },
                            message: 'must be boolean',
                          },
                        ];
                        return false;
                      }
                      var valid0 = _errs18 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.suspended_calls !== undefined) {
                        let data9 = data.suspended_calls;
                        const _errs20 = errors;
                        if (errors === _errs20) {
                          if (Array.isArray(data9)) {
                            var valid4 = true;
                            const len3 = data9.length;
                            for (let i3 = 0; i3 < len3; i3++) {
                              const _errs22 = errors;
                              if (typeof data9[i3] !== 'string') {
                                validate64.errors = [
                                  {
                                    instancePath: instancePath + '/suspended_calls/' + i3,
                                    schemaPath: '#/properties/suspended_calls/items/type',
                                    keyword: 'type',
                                    params: { type: 'string' },
                                    message: 'must be string',
                                  },
                                ];
                                return false;
                              }
                              var valid4 = _errs22 === errors;
                              if (!valid4) {
                                break;
                              }
                            }
                          } else {
                            validate64.errors = [
                              {
                                instancePath: instancePath + '/suspended_calls',
                                schemaPath: '#/properties/suspended_calls/type',
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
                        if (data.unavailable_capabilities !== undefined) {
                          let data11 = data.unavailable_capabilities;
                          const _errs24 = errors;
                          if (errors === _errs24) {
                            if (Array.isArray(data11)) {
                              var valid5 = true;
                              const len4 = data11.length;
                              for (let i4 = 0; i4 < len4; i4++) {
                                const _errs26 = errors;
                                if (typeof data11[i4] !== 'string') {
                                  validate64.errors = [
                                    {
                                      instancePath:
                                        instancePath + '/unavailable_capabilities/' + i4,
                                      schemaPath:
                                        '#/properties/unavailable_capabilities/items/type',
                                      keyword: 'type',
                                      params: { type: 'string' },
                                      message: 'must be string',
                                    },
                                  ];
                                  return false;
                                }
                                var valid5 = _errs26 === errors;
                                if (!valid5) {
                                  break;
                                }
                              }
                            } else {
                              validate64.errors = [
                                {
                                  instancePath: instancePath + '/unavailable_capabilities',
                                  schemaPath: '#/properties/unavailable_capabilities/type',
                                  keyword: 'type',
                                  params: { type: 'array' },
                                  message: 'must be array',
                                },
                              ];
                              return false;
                            }
                          }
                          var valid0 = _errs24 === errors;
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
    } else {
      validate64.errors = [
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
  validate64.errors = vErrors;
  return errors === 0;
}
validate64.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema26 = {
  additionalProperties: false,
  description:
    "The plan for a whole cross-domain recovery.\n\nA cross-domain restore must reach **one provable consistent version**,\nso the bundle plan is computed as a unit. It never staples each domain's own\nlast-known-good together and calls the result consistent: either a common\nrevision is provable, or the cross-domain restore is honestly disabled and the\nuser is offered a scoped default reset instead.",
  properties: {
    consistent_revision: {
      description: 'The common revision every affected domain can reach, when one exists.',
      minimum: 0,
      type: ['integer', 'null'],
      maximum: 9007199254740991,
    },
    consistent_set: {
      description:
        "The exact set the próven consistent version restores, when one exists.\n\nThis is the preview's own binding. `apply` consumes\n**this** list rather than re-selecting each domain's own latest history, so\nthe write cannot silently differ from what the user was shown. Each entry is\n`(module, revision, content digest of the target version)`.\n\nIt is empty exactly when no joint record proved a consistent set, which is\nalso when `consistent_revision` is `None`.",
      items: {
        maxItems: 3,
        minItems: 3,
        prefixItems: [
          { $ref: '#/$defs/ConfigModule' },
          { minimum: 0, type: 'integer', maximum: 9007199254740991 },
          { type: 'string' },
        ],
        type: 'array',
      },
      type: 'array',
    },
    domains: {
      description: 'Per-domain plans, so the user sees the whole scope before triggering.',
      items: { $ref: '#/$defs/DomainRecoveryPlan' },
      type: 'array',
    },
    inconsistency_code: {
      description: 'A bounded explanation when a consistent version cannot be proven.',
      type: ['string', 'null'],
    },
    modules: { items: { $ref: '#/$defs/ConfigModule' }, type: 'array' },
    options: { items: { $ref: '#/$defs/RecoveryOption' }, type: 'array' },
    preview_id: {
      description:
        'Core supplies this opaque binding; clients echo it without reimplementing hashing.',
      type: 'string',
    },
  },
  required: ['preview_id', 'modules', 'options', 'domains', 'consistent_set'],
  type: 'object',
};
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
  if (!(
    data === 'aura' ||
    data === 'models' ||
    data === 'tools' ||
    data === 'hooks' ||
    data === 'mcp' ||
    data === 'skills' ||
    data === 'commands' ||
    data === 'plugins' ||
    data === 'ui' ||
    data === 'prompt' ||
    data === 'security' ||
    data === 'browser' ||
    data === 'memory' ||
    data === 'diagnostics' ||
    data === 'update'
  )) {
    validate67.errors = [
      {
        instancePath,
        schemaPath: '#/enum',
        keyword: 'enum',
        params: { allowedValues: schema21.enum },
        message: 'must be equal to one of the allowed values',
      },
    ];
    return false;
  }
  validate67.errors = vErrors;
  return errors === 0;
}
validate67.evaluated = { dynamicProps: false, dynamicItems: false };
const schema28 = {
  additionalProperties: false,
  description:
    "What one domain's recovery would do, before anything is written.\n\nThis is the **preview**. It names the affected domain, the options with their\nreal availability, the versions that could be restored, the references that\nwould need rebinding, and the actual loss.",
  properties: {
    current_revision: {
      description: 'The revision currently served, if the domain serves anything.',
      minimum: 0,
      type: ['integer', 'null'],
      maximum: 9007199254740991,
    },
    degraded: {
      description: "The domain's current state, as the health registry reports it.",
      type: 'boolean',
    },
    fault_code: {
      description: 'The stable classification of the fault that made it degraded, if any.',
      type: ['string', 'null'],
    },
    loss: {
      description:
        'What the user actually loses by restoring the previous version. Stated so\nthe choice is informed rather than a guess.',
      items: { type: 'string' },
      type: 'array',
    },
    module: { $ref: '#/$defs/ConfigModule' },
    next_revision: {
      description: 'The revision the recovery result would carry: always `current + 1`.',
      minimum: 0,
      type: 'integer',
      maximum: 9007199254740991,
    },
    options: { items: { $ref: '#/$defs/RecoveryOption' }, type: 'array' },
    preview_id: {
      description:
        'The identity of the state this preview was computed from.\n\nA confirmation must be bound to what\nthe user was actually shown. Measured defect: a preview of a rev1 reset was\nstill applied after a legitimate save to dark/rev2, so the stale\nconfirmation overwrote a newer, valid configuration — `current_revision` was\nnot part of any check at commit time.\n\nThe caller echoes this back and the write refuses when the domain has moved\nsince. It is derived from the real facts the preview was built from, so a\ncaller cannot forge a preview for a state it was never shown.',
      type: 'string',
    },
    recoverable_versions: {
      description: 'The recorded versions, newest first, with their current-validity verdict.',
      items: { $ref: '#/$defs/RecoverableVersion' },
      type: 'array',
    },
    references_needing_rebinding: {
      description:
        'Settings keys that reference something outside the domain and would need\nrebinding after a restore or reset. Names only; never their values.',
      items: { type: 'string' },
      type: 'array',
    },
    scope_known: {
      description: "Whether the runtime could establish this domain's recovery scope at all.",
      type: 'boolean',
    },
  },
  required: [
    'module',
    'degraded',
    'options',
    'recoverable_versions',
    'next_revision',
    'references_needing_rebinding',
    'loss',
    'scope_known',
    'preview_id',
  ],
  type: 'object',
};
const func1 = Object.prototype.hasOwnProperty;
const schema29 = {
  additionalProperties: false,
  description:
    'Whether one recovery option is available, and exactly why not when it is not.\n\nAn unavailable option is **stated**, never silently omitted and never replaced\nby a different action. That is what "honestly disable the option" means.',
  properties: {
    action: { $ref: '#/$defs/ConfigRecoveryAction' },
    available: { type: 'boolean' },
    detail: {
      description: 'A bounded, path-free explanation for the user.',
      type: ['string', 'null'],
    },
    reason_code: {
      description:
        'A stable code when `available` is false or when the option carries a\ncaveat. No path, no document body, nothing the user supplied.',
      type: ['string', 'null'],
    },
  },
  required: ['action', 'available'],
  type: 'object',
};
function validate72(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate72.evaluated;
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
  if (typeof data !== 'string') {
    const err0 = {
      instancePath,
      schemaPath: '#/oneOf/0/type',
      keyword: 'type',
      params: { type: 'string' },
      message: 'must be string',
    };
    if (vErrors === null) {
      vErrors = [err0];
    } else {
      vErrors.push(err0);
    }
    errors++;
  }
  if ('retry' !== data) {
    const err1 = {
      instancePath,
      schemaPath: '#/oneOf/0/const',
      keyword: 'const',
      params: { allowedValue: 'retry' },
      message: 'must be equal to constant',
    };
    if (vErrors === null) {
      vErrors = [err1];
    } else {
      vErrors.push(err1);
    }
    errors++;
  }
  var _valid0 = _errs1 === errors;
  if (_valid0) {
    valid0 = true;
    passing0 = 0;
  }
  const _errs3 = errors;
  if (typeof data !== 'string') {
    const err2 = {
      instancePath,
      schemaPath: '#/oneOf/1/type',
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
  if ('restore_previous_valid' !== data) {
    const err3 = {
      instancePath,
      schemaPath: '#/oneOf/1/const',
      keyword: 'const',
      params: { allowedValue: 'restore_previous_valid' },
      message: 'must be equal to constant',
    };
    if (vErrors === null) {
      vErrors = [err3];
    } else {
      vErrors.push(err3);
    }
    errors++;
  }
  var _valid0 = _errs3 === errors;
  if (_valid0 && valid0) {
    valid0 = false;
    passing0 = [passing0, 1];
  } else {
    if (_valid0) {
      valid0 = true;
      passing0 = 1;
    }
    const _errs5 = errors;
    if (typeof data !== 'string') {
      const err4 = {
        instancePath,
        schemaPath: '#/oneOf/2/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      };
      if (vErrors === null) {
        vErrors = [err4];
      } else {
        vErrors.push(err4);
      }
      errors++;
    }
    if ('reset_to_default' !== data) {
      const err5 = {
        instancePath,
        schemaPath: '#/oneOf/2/const',
        keyword: 'const',
        params: { allowedValue: 'reset_to_default' },
        message: 'must be equal to constant',
      };
      if (vErrors === null) {
        vErrors = [err5];
      } else {
        vErrors.push(err5);
      }
      errors++;
    }
    var _valid0 = _errs5 === errors;
    if (_valid0 && valid0) {
      valid0 = false;
      passing0 = [passing0, 2];
    } else {
      if (_valid0) {
        valid0 = true;
        passing0 = 2;
      }
    }
  }
  if (!valid0) {
    const err6 = {
      instancePath,
      schemaPath: '#/oneOf',
      keyword: 'oneOf',
      params: { passingSchemas: passing0 },
      message: 'must match exactly one schema in oneOf',
    };
    if (vErrors === null) {
      vErrors = [err6];
    } else {
      vErrors.push(err6);
    }
    errors++;
    validate72.errors = vErrors;
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
  validate72.errors = vErrors;
  return errors === 0;
}
validate72.evaluated = { dynamicProps: false, dynamicItems: false };
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
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.action === undefined && (missing0 = 'action')) ||
        (data.available === undefined && (missing0 = 'available'))
      ) {
        validate71.errors = [
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
            key0 === 'action' ||
            key0 === 'available' ||
            key0 === 'detail' ||
            key0 === 'reason_code'
          )) {
            validate71.errors = [
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
            if (
              !validate72(data.action, {
                instancePath: instancePath + '/action',
                parentData: data,
                parentDataProperty: 'action',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate72.errors : vErrors.concat(validate72.errors);
              errors = vErrors.length;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.available !== undefined) {
              const _errs3 = errors;
              if (typeof data.available !== 'boolean') {
                validate71.errors = [
                  {
                    instancePath: instancePath + '/available',
                    schemaPath: '#/properties/available/type',
                    keyword: 'type',
                    params: { type: 'boolean' },
                    message: 'must be boolean',
                  },
                ];
                return false;
              }
              var valid0 = _errs3 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.detail !== undefined) {
                let data2 = data.detail;
                const _errs5 = errors;
                if (typeof data2 !== 'string' && data2 !== null) {
                  validate71.errors = [
                    {
                      instancePath: instancePath + '/detail',
                      schemaPath: '#/properties/detail/type',
                      keyword: 'type',
                      params: { type: schema29.properties.detail.type },
                      message: 'must be string,null',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs5 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.reason_code !== undefined) {
                  let data3 = data.reason_code;
                  const _errs7 = errors;
                  if (typeof data3 !== 'string' && data3 !== null) {
                    validate71.errors = [
                      {
                        instancePath: instancePath + '/reason_code',
                        schemaPath: '#/properties/reason_code/type',
                        keyword: 'type',
                        params: { type: schema29.properties.reason_code.type },
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
      validate71.errors = [
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
  validate71.errors = vErrors;
  return errors === 0;
}
validate71.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema31 = {
  additionalProperties: false,
  description: 'One recorded version the domain could be restored to.',
  properties: {
    passes_current_validation: {
      description: 'Whether this version still passes the **current** validator and permission.',
      type: 'boolean',
    },
    rejection_code: { description: 'A stable code when it does not.', type: ['string', 'null'] },
    revision: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
  },
  required: ['revision', 'passes_current_validation'],
  type: 'object',
};
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
        (data.revision === undefined && (missing0 = 'revision')) ||
        (data.passes_current_validation === undefined && (missing0 = 'passes_current_validation'))
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
            key0 === 'passes_current_validation' ||
            key0 === 'rejection_code' ||
            key0 === 'revision'
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
          if (data.passes_current_validation !== undefined) {
            const _errs2 = errors;
            if (typeof data.passes_current_validation !== 'boolean') {
              validate75.errors = [
                {
                  instancePath: instancePath + '/passes_current_validation',
                  schemaPath: '#/properties/passes_current_validation/type',
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
            if (data.rejection_code !== undefined) {
              let data1 = data.rejection_code;
              const _errs4 = errors;
              if (typeof data1 !== 'string' && data1 !== null) {
                validate75.errors = [
                  {
                    instancePath: instancePath + '/rejection_code',
                    schemaPath: '#/properties/rejection_code/type',
                    keyword: 'type',
                    params: { type: schema31.properties.rejection_code.type },
                    message: 'must be string,null',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.revision !== undefined) {
                let data2 = data.revision;
                const _errs6 = errors;
                if (!(
                  typeof data2 == 'number' &&
                  !(data2 % 1) &&
                  !isNaN(data2) &&
                  isFinite(data2)
                )) {
                  validate75.errors = [
                    {
                      instancePath: instancePath + '/revision',
                      schemaPath: '#/properties/revision/type',
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
                      validate75.errors = [
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
                      if (data2 < 0 || isNaN(data2)) {
                        validate75.errors = [
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
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
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
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.module === undefined && (missing0 = 'module')) ||
        (data.degraded === undefined && (missing0 = 'degraded')) ||
        (data.options === undefined && (missing0 = 'options')) ||
        (data.recoverable_versions === undefined && (missing0 = 'recoverable_versions')) ||
        (data.next_revision === undefined && (missing0 = 'next_revision')) ||
        (data.references_needing_rebinding === undefined &&
          (missing0 = 'references_needing_rebinding')) ||
        (data.loss === undefined && (missing0 = 'loss')) ||
        (data.scope_known === undefined && (missing0 = 'scope_known')) ||
        (data.preview_id === undefined && (missing0 = 'preview_id'))
      ) {
        validate69.errors = [
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
          if (!func1.call(schema28.properties, key0)) {
            validate69.errors = [
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
          if (data.current_revision !== undefined) {
            let data0 = data.current_revision;
            const _errs2 = errors;
            if (
              !(typeof data0 == 'number' && !(data0 % 1) && !isNaN(data0) && isFinite(data0)) &&
              data0 !== null
            ) {
              validate69.errors = [
                {
                  instancePath: instancePath + '/current_revision',
                  schemaPath: '#/properties/current_revision/type',
                  keyword: 'type',
                  params: { type: schema28.properties.current_revision.type },
                  message: 'must be integer,null',
                },
              ];
              return false;
            }
            if (errors === _errs2) {
              if (typeof data0 == 'number' && isFinite(data0)) {
                if (data0 > 9007199254740991 || isNaN(data0)) {
                  validate69.errors = [
                    {
                      instancePath: instancePath + '/current_revision',
                      schemaPath: '#/properties/current_revision/maximum',
                      keyword: 'maximum',
                      params: { comparison: '<=', limit: 9007199254740991 },
                      message: 'must be <= 9007199254740991',
                    },
                  ];
                  return false;
                } else {
                  if (data0 < 0 || isNaN(data0)) {
                    validate69.errors = [
                      {
                        instancePath: instancePath + '/current_revision',
                        schemaPath: '#/properties/current_revision/minimum',
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
            if (data.degraded !== undefined) {
              const _errs4 = errors;
              if (typeof data.degraded !== 'boolean') {
                validate69.errors = [
                  {
                    instancePath: instancePath + '/degraded',
                    schemaPath: '#/properties/degraded/type',
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
              if (data.fault_code !== undefined) {
                let data2 = data.fault_code;
                const _errs6 = errors;
                if (typeof data2 !== 'string' && data2 !== null) {
                  validate69.errors = [
                    {
                      instancePath: instancePath + '/fault_code',
                      schemaPath: '#/properties/fault_code/type',
                      keyword: 'type',
                      params: { type: schema28.properties.fault_code.type },
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
                if (data.loss !== undefined) {
                  let data3 = data.loss;
                  const _errs8 = errors;
                  if (errors === _errs8) {
                    if (Array.isArray(data3)) {
                      var valid1 = true;
                      const len0 = data3.length;
                      for (let i0 = 0; i0 < len0; i0++) {
                        const _errs10 = errors;
                        if (typeof data3[i0] !== 'string') {
                          validate69.errors = [
                            {
                              instancePath: instancePath + '/loss/' + i0,
                              schemaPath: '#/properties/loss/items/type',
                              keyword: 'type',
                              params: { type: 'string' },
                              message: 'must be string',
                            },
                          ];
                          return false;
                        }
                        var valid1 = _errs10 === errors;
                        if (!valid1) {
                          break;
                        }
                      }
                    } else {
                      validate69.errors = [
                        {
                          instancePath: instancePath + '/loss',
                          schemaPath: '#/properties/loss/type',
                          keyword: 'type',
                          params: { type: 'array' },
                          message: 'must be array',
                        },
                      ];
                      return false;
                    }
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.module !== undefined) {
                    const _errs12 = errors;
                    if (
                      !validate67(data.module, {
                        instancePath: instancePath + '/module',
                        parentData: data,
                        parentDataProperty: 'module',
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate67.errors : vErrors.concat(validate67.errors);
                      errors = vErrors.length;
                    }
                    var valid0 = _errs12 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.next_revision !== undefined) {
                      let data6 = data.next_revision;
                      const _errs13 = errors;
                      if (!(
                        typeof data6 == 'number' &&
                        !(data6 % 1) &&
                        !isNaN(data6) &&
                        isFinite(data6)
                      )) {
                        validate69.errors = [
                          {
                            instancePath: instancePath + '/next_revision',
                            schemaPath: '#/properties/next_revision/type',
                            keyword: 'type',
                            params: { type: 'integer' },
                            message: 'must be integer',
                          },
                        ];
                        return false;
                      }
                      if (errors === _errs13) {
                        if (typeof data6 == 'number' && isFinite(data6)) {
                          if (data6 > 9007199254740991 || isNaN(data6)) {
                            validate69.errors = [
                              {
                                instancePath: instancePath + '/next_revision',
                                schemaPath: '#/properties/next_revision/maximum',
                                keyword: 'maximum',
                                params: { comparison: '<=', limit: 9007199254740991 },
                                message: 'must be <= 9007199254740991',
                              },
                            ];
                            return false;
                          } else {
                            if (data6 < 0 || isNaN(data6)) {
                              validate69.errors = [
                                {
                                  instancePath: instancePath + '/next_revision',
                                  schemaPath: '#/properties/next_revision/minimum',
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
                      var valid0 = _errs13 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.options !== undefined) {
                        let data7 = data.options;
                        const _errs15 = errors;
                        if (errors === _errs15) {
                          if (Array.isArray(data7)) {
                            var valid2 = true;
                            const len1 = data7.length;
                            for (let i1 = 0; i1 < len1; i1++) {
                              const _errs17 = errors;
                              if (
                                !validate71(data7[i1], {
                                  instancePath: instancePath + '/options/' + i1,
                                  parentData: data7,
                                  parentDataProperty: i1,
                                  rootData,
                                  dynamicAnchors,
                                })
                              ) {
                                vErrors =
                                  vErrors === null
                                    ? validate71.errors
                                    : vErrors.concat(validate71.errors);
                                errors = vErrors.length;
                              }
                              var valid2 = _errs17 === errors;
                              if (!valid2) {
                                break;
                              }
                            }
                          } else {
                            validate69.errors = [
                              {
                                instancePath: instancePath + '/options',
                                schemaPath: '#/properties/options/type',
                                keyword: 'type',
                                params: { type: 'array' },
                                message: 'must be array',
                              },
                            ];
                            return false;
                          }
                        }
                        var valid0 = _errs15 === errors;
                      } else {
                        var valid0 = true;
                      }
                      if (valid0) {
                        if (data.preview_id !== undefined) {
                          const _errs18 = errors;
                          if (typeof data.preview_id !== 'string') {
                            validate69.errors = [
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
                          var valid0 = _errs18 === errors;
                        } else {
                          var valid0 = true;
                        }
                        if (valid0) {
                          if (data.recoverable_versions !== undefined) {
                            let data10 = data.recoverable_versions;
                            const _errs20 = errors;
                            if (errors === _errs20) {
                              if (Array.isArray(data10)) {
                                var valid3 = true;
                                const len2 = data10.length;
                                for (let i2 = 0; i2 < len2; i2++) {
                                  const _errs22 = errors;
                                  if (
                                    !validate75(data10[i2], {
                                      instancePath: instancePath + '/recoverable_versions/' + i2,
                                      parentData: data10,
                                      parentDataProperty: i2,
                                      rootData,
                                      dynamicAnchors,
                                    })
                                  ) {
                                    vErrors =
                                      vErrors === null
                                        ? validate75.errors
                                        : vErrors.concat(validate75.errors);
                                    errors = vErrors.length;
                                  }
                                  var valid3 = _errs22 === errors;
                                  if (!valid3) {
                                    break;
                                  }
                                }
                              } else {
                                validate69.errors = [
                                  {
                                    instancePath: instancePath + '/recoverable_versions',
                                    schemaPath: '#/properties/recoverable_versions/type',
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
                            if (data.references_needing_rebinding !== undefined) {
                              let data12 = data.references_needing_rebinding;
                              const _errs23 = errors;
                              if (errors === _errs23) {
                                if (Array.isArray(data12)) {
                                  var valid4 = true;
                                  const len3 = data12.length;
                                  for (let i3 = 0; i3 < len3; i3++) {
                                    const _errs25 = errors;
                                    if (typeof data12[i3] !== 'string') {
                                      validate69.errors = [
                                        {
                                          instancePath:
                                            instancePath + '/references_needing_rebinding/' + i3,
                                          schemaPath:
                                            '#/properties/references_needing_rebinding/items/type',
                                          keyword: 'type',
                                          params: { type: 'string' },
                                          message: 'must be string',
                                        },
                                      ];
                                      return false;
                                    }
                                    var valid4 = _errs25 === errors;
                                    if (!valid4) {
                                      break;
                                    }
                                  }
                                } else {
                                  validate69.errors = [
                                    {
                                      instancePath: instancePath + '/references_needing_rebinding',
                                      schemaPath: '#/properties/references_needing_rebinding/type',
                                      keyword: 'type',
                                      params: { type: 'array' },
                                      message: 'must be array',
                                    },
                                  ];
                                  return false;
                                }
                              }
                              var valid0 = _errs23 === errors;
                            } else {
                              var valid0 = true;
                            }
                            if (valid0) {
                              if (data.scope_known !== undefined) {
                                const _errs27 = errors;
                                if (typeof data.scope_known !== 'boolean') {
                                  validate69.errors = [
                                    {
                                      instancePath: instancePath + '/scope_known',
                                      schemaPath: '#/properties/scope_known/type',
                                      keyword: 'type',
                                      params: { type: 'boolean' },
                                      message: 'must be boolean',
                                    },
                                  ];
                                  return false;
                                }
                                var valid0 = _errs27 === errors;
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
    } else {
      validate69.errors = [
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
  validate69.errors = vErrors;
  return errors === 0;
}
validate69.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate66(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate66.evaluated;
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
        (data.preview_id === undefined && (missing0 = 'preview_id')) ||
        (data.modules === undefined && (missing0 = 'modules')) ||
        (data.options === undefined && (missing0 = 'options')) ||
        (data.domains === undefined && (missing0 = 'domains')) ||
        (data.consistent_set === undefined && (missing0 = 'consistent_set'))
      ) {
        validate66.errors = [
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
            key0 === 'consistent_revision' ||
            key0 === 'consistent_set' ||
            key0 === 'domains' ||
            key0 === 'inconsistency_code' ||
            key0 === 'modules' ||
            key0 === 'options' ||
            key0 === 'preview_id'
          )) {
            validate66.errors = [
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
          if (data.consistent_revision !== undefined) {
            let data0 = data.consistent_revision;
            const _errs2 = errors;
            if (
              !(typeof data0 == 'number' && !(data0 % 1) && !isNaN(data0) && isFinite(data0)) &&
              data0 !== null
            ) {
              validate66.errors = [
                {
                  instancePath: instancePath + '/consistent_revision',
                  schemaPath: '#/properties/consistent_revision/type',
                  keyword: 'type',
                  params: { type: schema26.properties.consistent_revision.type },
                  message: 'must be integer,null',
                },
              ];
              return false;
            }
            if (errors === _errs2) {
              if (typeof data0 == 'number' && isFinite(data0)) {
                if (data0 > 9007199254740991 || isNaN(data0)) {
                  validate66.errors = [
                    {
                      instancePath: instancePath + '/consistent_revision',
                      schemaPath: '#/properties/consistent_revision/maximum',
                      keyword: 'maximum',
                      params: { comparison: '<=', limit: 9007199254740991 },
                      message: 'must be <= 9007199254740991',
                    },
                  ];
                  return false;
                } else {
                  if (data0 < 0 || isNaN(data0)) {
                    validate66.errors = [
                      {
                        instancePath: instancePath + '/consistent_revision',
                        schemaPath: '#/properties/consistent_revision/minimum',
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
            if (data.consistent_set !== undefined) {
              let data1 = data.consistent_set;
              const _errs4 = errors;
              if (errors === _errs4) {
                if (Array.isArray(data1)) {
                  var valid1 = true;
                  const len0 = data1.length;
                  for (let i0 = 0; i0 < len0; i0++) {
                    let data2 = data1[i0];
                    const _errs6 = errors;
                    if (errors === _errs6) {
                      if (Array.isArray(data2)) {
                        if (data2.length > 3) {
                          validate66.errors = [
                            {
                              instancePath: instancePath + '/consistent_set/' + i0,
                              schemaPath: '#/properties/consistent_set/items/maxItems',
                              keyword: 'maxItems',
                              params: { limit: 3 },
                              message: 'must NOT have more than 3 items',
                            },
                          ];
                          return false;
                        } else {
                          if (data2.length < 3) {
                            validate66.errors = [
                              {
                                instancePath: instancePath + '/consistent_set/' + i0,
                                schemaPath: '#/properties/consistent_set/items/minItems',
                                keyword: 'minItems',
                                params: { limit: 3 },
                                message: 'must NOT have fewer than 3 items',
                              },
                            ];
                            return false;
                          } else {
                            const len1 = data2.length;
                            if (len1 > 0) {
                              const _errs8 = errors;
                              if (
                                !validate67(data2[0], {
                                  instancePath: instancePath + '/consistent_set/' + i0 + '/0',
                                  parentData: data2,
                                  parentDataProperty: 0,
                                  rootData,
                                  dynamicAnchors,
                                })
                              ) {
                                vErrors =
                                  vErrors === null
                                    ? validate67.errors
                                    : vErrors.concat(validate67.errors);
                                errors = vErrors.length;
                              }
                              var valid2 = _errs8 === errors;
                            }
                            if (valid2) {
                              if (len1 > 1) {
                                let data4 = data2[1];
                                const _errs9 = errors;
                                if (!(
                                  typeof data4 == 'number' &&
                                  !(data4 % 1) &&
                                  !isNaN(data4) &&
                                  isFinite(data4)
                                )) {
                                  validate66.errors = [
                                    {
                                      instancePath: instancePath + '/consistent_set/' + i0 + '/1',
                                      schemaPath:
                                        '#/properties/consistent_set/items/prefixItems/1/type',
                                      keyword: 'type',
                                      params: { type: 'integer' },
                                      message: 'must be integer',
                                    },
                                  ];
                                  return false;
                                }
                                if (errors === _errs9) {
                                  if (typeof data4 == 'number' && isFinite(data4)) {
                                    if (data4 > 9007199254740991 || isNaN(data4)) {
                                      validate66.errors = [
                                        {
                                          instancePath:
                                            instancePath + '/consistent_set/' + i0 + '/1',
                                          schemaPath:
                                            '#/properties/consistent_set/items/prefixItems/1/maximum',
                                          keyword: 'maximum',
                                          params: { comparison: '<=', limit: 9007199254740991 },
                                          message: 'must be <= 9007199254740991',
                                        },
                                      ];
                                      return false;
                                    } else {
                                      if (data4 < 0 || isNaN(data4)) {
                                        validate66.errors = [
                                          {
                                            instancePath:
                                              instancePath + '/consistent_set/' + i0 + '/1',
                                            schemaPath:
                                              '#/properties/consistent_set/items/prefixItems/1/minimum',
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
                                var valid2 = _errs9 === errors;
                              }
                              if (valid2) {
                                if (len1 > 2) {
                                  const _errs11 = errors;
                                  if (typeof data2[2] !== 'string') {
                                    validate66.errors = [
                                      {
                                        instancePath: instancePath + '/consistent_set/' + i0 + '/2',
                                        schemaPath:
                                          '#/properties/consistent_set/items/prefixItems/2/type',
                                        keyword: 'type',
                                        params: { type: 'string' },
                                        message: 'must be string',
                                      },
                                    ];
                                    return false;
                                  }
                                  var valid2 = _errs11 === errors;
                                }
                              }
                            }
                          }
                        }
                      } else {
                        validate66.errors = [
                          {
                            instancePath: instancePath + '/consistent_set/' + i0,
                            schemaPath: '#/properties/consistent_set/items/type',
                            keyword: 'type',
                            params: { type: 'array' },
                            message: 'must be array',
                          },
                        ];
                        return false;
                      }
                    }
                    var valid1 = _errs6 === errors;
                    if (!valid1) {
                      break;
                    }
                  }
                } else {
                  validate66.errors = [
                    {
                      instancePath: instancePath + '/consistent_set',
                      schemaPath: '#/properties/consistent_set/type',
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
              if (data.domains !== undefined) {
                let data6 = data.domains;
                const _errs13 = errors;
                if (errors === _errs13) {
                  if (Array.isArray(data6)) {
                    var valid3 = true;
                    const len2 = data6.length;
                    for (let i1 = 0; i1 < len2; i1++) {
                      const _errs15 = errors;
                      if (
                        !validate69(data6[i1], {
                          instancePath: instancePath + '/domains/' + i1,
                          parentData: data6,
                          parentDataProperty: i1,
                          rootData,
                          dynamicAnchors,
                        })
                      ) {
                        vErrors =
                          vErrors === null ? validate69.errors : vErrors.concat(validate69.errors);
                        errors = vErrors.length;
                      }
                      var valid3 = _errs15 === errors;
                      if (!valid3) {
                        break;
                      }
                    }
                  } else {
                    validate66.errors = [
                      {
                        instancePath: instancePath + '/domains',
                        schemaPath: '#/properties/domains/type',
                        keyword: 'type',
                        params: { type: 'array' },
                        message: 'must be array',
                      },
                    ];
                    return false;
                  }
                }
                var valid0 = _errs13 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.inconsistency_code !== undefined) {
                  let data8 = data.inconsistency_code;
                  const _errs16 = errors;
                  if (typeof data8 !== 'string' && data8 !== null) {
                    validate66.errors = [
                      {
                        instancePath: instancePath + '/inconsistency_code',
                        schemaPath: '#/properties/inconsistency_code/type',
                        keyword: 'type',
                        params: { type: schema26.properties.inconsistency_code.type },
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
                  if (data.modules !== undefined) {
                    let data9 = data.modules;
                    const _errs18 = errors;
                    if (errors === _errs18) {
                      if (Array.isArray(data9)) {
                        var valid4 = true;
                        const len3 = data9.length;
                        for (let i2 = 0; i2 < len3; i2++) {
                          const _errs20 = errors;
                          if (
                            !validate67(data9[i2], {
                              instancePath: instancePath + '/modules/' + i2,
                              parentData: data9,
                              parentDataProperty: i2,
                              rootData,
                              dynamicAnchors,
                            })
                          ) {
                            vErrors =
                              vErrors === null
                                ? validate67.errors
                                : vErrors.concat(validate67.errors);
                            errors = vErrors.length;
                          }
                          var valid4 = _errs20 === errors;
                          if (!valid4) {
                            break;
                          }
                        }
                      } else {
                        validate66.errors = [
                          {
                            instancePath: instancePath + '/modules',
                            schemaPath: '#/properties/modules/type',
                            keyword: 'type',
                            params: { type: 'array' },
                            message: 'must be array',
                          },
                        ];
                        return false;
                      }
                    }
                    var valid0 = _errs18 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.options !== undefined) {
                      let data11 = data.options;
                      const _errs21 = errors;
                      if (errors === _errs21) {
                        if (Array.isArray(data11)) {
                          var valid5 = true;
                          const len4 = data11.length;
                          for (let i3 = 0; i3 < len4; i3++) {
                            const _errs23 = errors;
                            if (
                              !validate71(data11[i3], {
                                instancePath: instancePath + '/options/' + i3,
                                parentData: data11,
                                parentDataProperty: i3,
                                rootData,
                                dynamicAnchors,
                              })
                            ) {
                              vErrors =
                                vErrors === null
                                  ? validate71.errors
                                  : vErrors.concat(validate71.errors);
                              errors = vErrors.length;
                            }
                            var valid5 = _errs23 === errors;
                            if (!valid5) {
                              break;
                            }
                          }
                        } else {
                          validate66.errors = [
                            {
                              instancePath: instancePath + '/options',
                              schemaPath: '#/properties/options/type',
                              keyword: 'type',
                              params: { type: 'array' },
                              message: 'must be array',
                            },
                          ];
                          return false;
                        }
                      }
                      var valid0 = _errs21 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.preview_id !== undefined) {
                        const _errs24 = errors;
                        if (typeof data.preview_id !== 'string') {
                          validate66.errors = [
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
                        var valid0 = _errs24 === errors;
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
    } else {
      validate66.errors = [
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
  validate66.errors = vErrors;
  return errors === 0;
}
validate66.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema32 = {
  additionalProperties: false,
  properties: {
    action: { $ref: '#/$defs/ConfigRecoveryAction' },
    outcomes: { items: { $ref: '#/$defs/RecoveryOutcome' }, type: 'array' },
    reason_code: {
      description: 'A stable code when the attempt did not fully apply.',
      type: ['string', 'null'],
    },
    restored_capabilities: {
      description: 'The capabilities whose service is **really present** after this attempt.',
      items: { type: 'string' },
      type: 'array',
    },
    unavailable_capabilities: {
      default: [],
      description:
        'The capabilities still unavailable afterwards, each with its stable reason.\n\nThe previous report had nowhere to put this, so a\ncapability whose construction genuinely failed was indistinguishable from one that came\nback. The user saw a cleared fault marker and concluded the feature worked.',
      items: { $ref: '#/$defs/UnavailableCapability' },
      type: 'array',
    },
  },
  required: ['action', 'outcomes', 'restored_capabilities'],
  type: 'object',
};
const schema33 = {
  additionalProperties: false,
  description: 'What one recovery action really did.',
  properties: {
    action: { $ref: '#/$defs/ConfigRecoveryAction' },
    applied: { type: 'boolean' },
    module: { $ref: '#/$defs/ConfigModule' },
    reason_code: {
      description: 'A stable code when the action did not apply.',
      type: ['string', 'null'],
    },
    revision: {
      description: 'The new revision the domain now serves, when the action applied.',
      minimum: 0,
      type: ['integer', 'null'],
      maximum: 9007199254740991,
    },
    source: {
      anyOf: [{ $ref: '#/$defs/ConfigSource' }, { type: 'null' }],
      description: 'The source the recovered snapshot came from.',
    },
  },
  required: ['module', 'action', 'applied'],
  type: 'object',
};
const schema34 = {
  description:
    "Where a domain's currently-served snapshot came from.\n\nSerializable because a recovery outcome reports it to the user, and the\noutcome is part of a projection. The wire form is the stable snake_case key, so\na renderer can name the source without a second mapping table.",
  enum: ['default', 'file', 'last_known_good'],
  type: 'string',
};
function validate86(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate86.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate86.errors = [
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
  if (!(data === 'default' || data === 'file' || data === 'last_known_good')) {
    validate86.errors = [
      {
        instancePath,
        schemaPath: '#/enum',
        keyword: 'enum',
        params: { allowedValues: schema34.enum },
        message: 'must be equal to one of the allowed values',
      },
    ];
    return false;
  }
  validate86.errors = vErrors;
  return errors === 0;
}
validate86.evaluated = { dynamicProps: false, dynamicItems: false };
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
        (data.module === undefined && (missing0 = 'module')) ||
        (data.action === undefined && (missing0 = 'action')) ||
        (data.applied === undefined && (missing0 = 'applied'))
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
            key0 === 'action' ||
            key0 === 'applied' ||
            key0 === 'module' ||
            key0 === 'reason_code' ||
            key0 === 'revision' ||
            key0 === 'source'
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
          if (data.action !== undefined) {
            const _errs2 = errors;
            if (
              !validate72(data.action, {
                instancePath: instancePath + '/action',
                parentData: data,
                parentDataProperty: 'action',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate72.errors : vErrors.concat(validate72.errors);
              errors = vErrors.length;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.applied !== undefined) {
              const _errs3 = errors;
              if (typeof data.applied !== 'boolean') {
                validate83.errors = [
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
              var valid0 = _errs3 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.module !== undefined) {
                const _errs5 = errors;
                if (
                  !validate67(data.module, {
                    instancePath: instancePath + '/module',
                    parentData: data,
                    parentDataProperty: 'module',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate67.errors : vErrors.concat(validate67.errors);
                  errors = vErrors.length;
                }
                var valid0 = _errs5 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.reason_code !== undefined) {
                  let data3 = data.reason_code;
                  const _errs6 = errors;
                  if (typeof data3 !== 'string' && data3 !== null) {
                    validate83.errors = [
                      {
                        instancePath: instancePath + '/reason_code',
                        schemaPath: '#/properties/reason_code/type',
                        keyword: 'type',
                        params: { type: schema33.properties.reason_code.type },
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
                  if (data.revision !== undefined) {
                    let data4 = data.revision;
                    const _errs8 = errors;
                    if (
                      !(
                        typeof data4 == 'number' &&
                        !(data4 % 1) &&
                        !isNaN(data4) &&
                        isFinite(data4)
                      ) &&
                      data4 !== null
                    ) {
                      validate83.errors = [
                        {
                          instancePath: instancePath + '/revision',
                          schemaPath: '#/properties/revision/type',
                          keyword: 'type',
                          params: { type: schema33.properties.revision.type },
                          message: 'must be integer,null',
                        },
                      ];
                      return false;
                    }
                    if (errors === _errs8) {
                      if (typeof data4 == 'number' && isFinite(data4)) {
                        if (data4 > 9007199254740991 || isNaN(data4)) {
                          validate83.errors = [
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
                          if (data4 < 0 || isNaN(data4)) {
                            validate83.errors = [
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
                    var valid0 = _errs8 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.source !== undefined) {
                      let data5 = data.source;
                      const _errs10 = errors;
                      const _errs11 = errors;
                      let valid1 = false;
                      const _errs12 = errors;
                      if (
                        !validate86(data5, {
                          instancePath: instancePath + '/source',
                          parentData: data,
                          parentDataProperty: 'source',
                          rootData,
                          dynamicAnchors,
                        })
                      ) {
                        vErrors =
                          vErrors === null ? validate86.errors : vErrors.concat(validate86.errors);
                        errors = vErrors.length;
                      }
                      var _valid0 = _errs12 === errors;
                      valid1 = valid1 || _valid0;
                      const _errs13 = errors;
                      if (data5 !== null) {
                        const err0 = {
                          instancePath: instancePath + '/source',
                          schemaPath: '#/properties/source/anyOf/1/type',
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
                      valid1 = valid1 || _valid0;
                      if (!valid1) {
                        const err1 = {
                          instancePath: instancePath + '/source',
                          schemaPath: '#/properties/source/anyOf',
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
                        validate83.errors = vErrors;
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
    capability: { description: 'The capability, by stable key.', type: 'string' },
    reason: {
      description: 'Why it is still unavailable, in bounded, path-free terms.',
      type: 'string',
    },
  },
  required: ['capability', 'reason'],
  type: 'object',
};
function validate89(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate89.evaluated;
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
        (data.capability === undefined && (missing0 = 'capability')) ||
        (data.reason === undefined && (missing0 = 'reason'))
      ) {
        validate89.errors = [
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
          if (!(key0 === 'capability' || key0 === 'reason')) {
            validate89.errors = [
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
          if (data.capability !== undefined) {
            const _errs2 = errors;
            if (typeof data.capability !== 'string') {
              validate89.errors = [
                {
                  instancePath: instancePath + '/capability',
                  schemaPath: '#/properties/capability/type',
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
                validate89.errors = [
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
      validate89.errors = [
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
  validate89.errors = vErrors;
  return errors === 0;
}
validate89.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate81(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate81.evaluated;
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
        (data.action === undefined && (missing0 = 'action')) ||
        (data.outcomes === undefined && (missing0 = 'outcomes')) ||
        (data.restored_capabilities === undefined && (missing0 = 'restored_capabilities'))
      ) {
        validate81.errors = [
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
            key0 === 'action' ||
            key0 === 'outcomes' ||
            key0 === 'reason_code' ||
            key0 === 'restored_capabilities' ||
            key0 === 'unavailable_capabilities'
          )) {
            validate81.errors = [
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
            if (
              !validate72(data.action, {
                instancePath: instancePath + '/action',
                parentData: data,
                parentDataProperty: 'action',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate72.errors : vErrors.concat(validate72.errors);
              errors = vErrors.length;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.outcomes !== undefined) {
              let data1 = data.outcomes;
              const _errs3 = errors;
              if (errors === _errs3) {
                if (Array.isArray(data1)) {
                  var valid1 = true;
                  const len0 = data1.length;
                  for (let i0 = 0; i0 < len0; i0++) {
                    const _errs5 = errors;
                    if (
                      !validate83(data1[i0], {
                        instancePath: instancePath + '/outcomes/' + i0,
                        parentData: data1,
                        parentDataProperty: i0,
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate83.errors : vErrors.concat(validate83.errors);
                      errors = vErrors.length;
                    }
                    var valid1 = _errs5 === errors;
                    if (!valid1) {
                      break;
                    }
                  }
                } else {
                  validate81.errors = [
                    {
                      instancePath: instancePath + '/outcomes',
                      schemaPath: '#/properties/outcomes/type',
                      keyword: 'type',
                      params: { type: 'array' },
                      message: 'must be array',
                    },
                  ];
                  return false;
                }
              }
              var valid0 = _errs3 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.reason_code !== undefined) {
                let data3 = data.reason_code;
                const _errs6 = errors;
                if (typeof data3 !== 'string' && data3 !== null) {
                  validate81.errors = [
                    {
                      instancePath: instancePath + '/reason_code',
                      schemaPath: '#/properties/reason_code/type',
                      keyword: 'type',
                      params: { type: schema32.properties.reason_code.type },
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
                if (data.restored_capabilities !== undefined) {
                  let data4 = data.restored_capabilities;
                  const _errs8 = errors;
                  if (errors === _errs8) {
                    if (Array.isArray(data4)) {
                      var valid2 = true;
                      const len1 = data4.length;
                      for (let i1 = 0; i1 < len1; i1++) {
                        const _errs10 = errors;
                        if (typeof data4[i1] !== 'string') {
                          validate81.errors = [
                            {
                              instancePath: instancePath + '/restored_capabilities/' + i1,
                              schemaPath: '#/properties/restored_capabilities/items/type',
                              keyword: 'type',
                              params: { type: 'string' },
                              message: 'must be string',
                            },
                          ];
                          return false;
                        }
                        var valid2 = _errs10 === errors;
                        if (!valid2) {
                          break;
                        }
                      }
                    } else {
                      validate81.errors = [
                        {
                          instancePath: instancePath + '/restored_capabilities',
                          schemaPath: '#/properties/restored_capabilities/type',
                          keyword: 'type',
                          params: { type: 'array' },
                          message: 'must be array',
                        },
                      ];
                      return false;
                    }
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.unavailable_capabilities !== undefined) {
                    let data6 = data.unavailable_capabilities;
                    const _errs12 = errors;
                    if (errors === _errs12) {
                      if (Array.isArray(data6)) {
                        var valid3 = true;
                        const len2 = data6.length;
                        for (let i2 = 0; i2 < len2; i2++) {
                          const _errs14 = errors;
                          if (
                            !validate89(data6[i2], {
                              instancePath: instancePath + '/unavailable_capabilities/' + i2,
                              parentData: data6,
                              parentDataProperty: i2,
                              rootData,
                              dynamicAnchors,
                            })
                          ) {
                            vErrors =
                              vErrors === null
                                ? validate89.errors
                                : vErrors.concat(validate89.errors);
                            errors = vErrors.length;
                          }
                          var valid3 = _errs14 === errors;
                          if (!valid3) {
                            break;
                          }
                        }
                      } else {
                        validate81.errors = [
                          {
                            instancePath: instancePath + '/unavailable_capabilities',
                            schemaPath: '#/properties/unavailable_capabilities/type',
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
                }
              }
            }
          }
        }
      }
    } else {
      validate81.errors = [
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
  validate81.errors = vErrors;
  return errors === 0;
}
validate81.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate63(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate63.evaluated;
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
            if ('config.recovery.notice' !== data.command) {
              const err2 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/0/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'config.recovery.notice' },
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
                !validate64(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate64.errors : vErrors.concat(validate64.errors);
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
            if ('config.recovery.plan' !== data.command) {
              const err6 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/1/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'config.recovery.plan' },
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
                !validate66(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate66.errors : vErrors.concat(validate66.errors);
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
            if ('config.recovery.apply' !== data.command) {
              const err10 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/2/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'config.recovery.apply' },
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
                !validate81(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate81.errors : vErrors.concat(validate81.errors);
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
  if (!valid0) {
    const err12 = {
      instancePath,
      schemaPath: '#/anyOf',
      keyword: 'anyOf',
      params: {},
      message: 'must match a schema in anyOf',
    };
    if (vErrors === null) {
      vErrors = [err12];
    } else {
      vErrors.push(err12);
    }
    errors++;
    validate63.errors = vErrors;
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
  validate63.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate63.evaluated = { dynamicProps: true, dynamicItems: false };
