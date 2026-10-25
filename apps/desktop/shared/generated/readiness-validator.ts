// @ts-nocheck
/* eslint-disable */
/** Generated from Rust schema using AJV standalone. Do not edit. */
'use strict';
export const validate = validate20;
export default validate20;
const schema31 = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $ref: '#/$defs/RuntimeReadiness',
  $defs: {
    RuntimeReadiness: {
      additionalProperties: false,
      description:
        'Core integrity, service availability and optional execution facilities are\nseparate facts. Backend readiness does not grant permission to dispatch.',
      properties: {
        backends: { items: { $ref: '#/$defs/BackendStatus' }, type: 'array' },
        capabilities: { items: { $ref: '#/$defs/CapabilityStatus' }, type: 'array' },
        core_health: { $ref: '#/$defs/CoreHealth' },
        extensions: { items: { $ref: '#/$defs/ExtensionStatus' }, type: 'array' },
        revision: { $ref: '#/$defs/Revision' },
        schema_version: { $ref: '#/$defs/ToolWireVersionV1' },
      },
      required: [
        'schema_version',
        'revision',
        'core_health',
        'capabilities',
        'backends',
        'extensions',
      ],
      type: 'object',
    },
    BackendStatus: {
      additionalProperties: false,
      properties: {
        backend: { type: 'string' },
        lifecycle: { $ref: '#/$defs/BackendLifecycle' },
        readiness: { $ref: '#/$defs/BackendReadiness' },
      },
      required: ['backend', 'lifecycle', 'readiness'],
      type: 'object',
    },
    BackendLifecycle: { enum: ['dormant', 'starting', 'ready', 'degraded'], type: 'string' },
    BackendReadiness: {
      oneOf: [
        {
          additionalProperties: false,
          properties: { readiness: { const: 'ready', type: 'string' } },
          required: ['readiness'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            readiness: { const: 'unavailable', type: 'string' },
            reason: { type: 'string' },
          },
          required: ['readiness', 'reason'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            readiness: { const: 'setup_required', type: 'string' },
            reason: { type: 'string' },
          },
          required: ['readiness', 'reason'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            readiness: { const: 'repair_required', type: 'string' },
            reason: { type: 'string' },
          },
          required: ['readiness', 'reason'],
          type: 'object',
        },
      ],
    },
    CapabilityStatus: {
      additionalProperties: false,
      properties: {
        capability: { type: 'string' },
        readiness: { $ref: '#/$defs/CapabilityReadiness' },
        reason_code: { type: 'string' },
      },
      required: ['capability', 'readiness', 'reason_code'],
      type: 'object',
    },
    CapabilityReadiness: { enum: ['ready', 'unavailable'], type: 'string' },
    CoreHealth: { enum: ['starting', 'ready', 'recovery_required'], type: 'string' },
    ExtensionStatus: {
      additionalProperties: false,
      properties: {
        extension: { type: 'string' },
        readiness: { $ref: '#/$defs/ExtensionReadiness' },
        reason_code: { type: 'string' },
      },
      required: ['extension', 'readiness', 'reason_code'],
      type: 'object',
    },
    ExtensionReadiness: { enum: ['disabled', 'ready', 'degraded'], type: 'string' },
    Revision: { type: 'string' },
    ToolWireVersionV1: { const: 1, type: 'integer' },
  },
};
const schema32 = {
  additionalProperties: false,
  description:
    'Core integrity, service availability and optional execution facilities are\nseparate facts. Backend readiness does not grant permission to dispatch.',
  properties: {
    backends: { items: { $ref: '#/$defs/BackendStatus' }, type: 'array' },
    capabilities: { items: { $ref: '#/$defs/CapabilityStatus' }, type: 'array' },
    core_health: { $ref: '#/$defs/CoreHealth' },
    extensions: { items: { $ref: '#/$defs/ExtensionStatus' }, type: 'array' },
    revision: { $ref: '#/$defs/Revision' },
    schema_version: { $ref: '#/$defs/ToolWireVersionV1' },
  },
  required: ['schema_version', 'revision', 'core_health', 'capabilities', 'backends', 'extensions'],
  type: 'object',
};
const schema38 = { enum: ['starting', 'ready', 'recovery_required'], type: 'string' };
const schema41 = { type: 'string' };
const schema42 = { const: 1, type: 'integer' };
const schema33 = {
  additionalProperties: false,
  properties: {
    backend: { type: 'string' },
    lifecycle: { $ref: '#/$defs/BackendLifecycle' },
    readiness: { $ref: '#/$defs/BackendReadiness' },
  },
  required: ['backend', 'lifecycle', 'readiness'],
  type: 'object',
};
const schema34 = { enum: ['dormant', 'starting', 'ready', 'degraded'], type: 'string' };
const schema35 = {
  oneOf: [
    {
      additionalProperties: false,
      properties: { readiness: { const: 'ready', type: 'string' } },
      required: ['readiness'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: {
        readiness: { const: 'unavailable', type: 'string' },
        reason: { type: 'string' },
      },
      required: ['readiness', 'reason'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: {
        readiness: { const: 'setup_required', type: 'string' },
        reason: { type: 'string' },
      },
      required: ['readiness', 'reason'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: {
        readiness: { const: 'repair_required', type: 'string' },
        reason: { type: 'string' },
      },
      required: ['readiness', 'reason'],
      type: 'object',
    },
  ],
};
function validate22(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate22.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (data && typeof data == 'object' && !Array.isArray(data)) {
    if (data.backend === undefined) {
      const err0 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'backend' },
        message: "must have required property '" + 'backend' + "'",
      };
      if (vErrors === null) {
        vErrors = [err0];
      } else {
        vErrors.push(err0);
      }
      errors++;
    }
    if (data.lifecycle === undefined) {
      const err1 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'lifecycle' },
        message: "must have required property '" + 'lifecycle' + "'",
      };
      if (vErrors === null) {
        vErrors = [err1];
      } else {
        vErrors.push(err1);
      }
      errors++;
    }
    if (data.readiness === undefined) {
      const err2 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'readiness' },
        message: "must have required property '" + 'readiness' + "'",
      };
      if (vErrors === null) {
        vErrors = [err2];
      } else {
        vErrors.push(err2);
      }
      errors++;
    }
    for (const key0 in data) {
      if (!(key0 === 'backend' || key0 === 'lifecycle' || key0 === 'readiness')) {
        const err3 = {
          instancePath,
          schemaPath: '#/additionalProperties',
          keyword: 'additionalProperties',
          params: { additionalProperty: key0 },
          message: 'must NOT have additional properties',
        };
        if (vErrors === null) {
          vErrors = [err3];
        } else {
          vErrors.push(err3);
        }
        errors++;
      }
    }
    if (data.backend !== undefined) {
      if (typeof data.backend !== 'string') {
        const err4 = {
          instancePath: instancePath + '/backend',
          schemaPath: '#/properties/backend/type',
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
    }
    if (data.lifecycle !== undefined) {
      let data1 = data.lifecycle;
      if (typeof data1 !== 'string') {
        const err5 = {
          instancePath: instancePath + '/lifecycle',
          schemaPath: '#/$defs/BackendLifecycle/type',
          keyword: 'type',
          params: { type: 'string' },
          message: 'must be string',
        };
        if (vErrors === null) {
          vErrors = [err5];
        } else {
          vErrors.push(err5);
        }
        errors++;
      }
      if (!(
        data1 === 'dormant' ||
        data1 === 'starting' ||
        data1 === 'ready' ||
        data1 === 'degraded'
      )) {
        const err6 = {
          instancePath: instancePath + '/lifecycle',
          schemaPath: '#/$defs/BackendLifecycle/enum',
          keyword: 'enum',
          params: { allowedValues: schema34.enum },
          message: 'must be equal to one of the allowed values',
        };
        if (vErrors === null) {
          vErrors = [err6];
        } else {
          vErrors.push(err6);
        }
        errors++;
      }
    }
    if (data.readiness !== undefined) {
      let data2 = data.readiness;
      const _errs9 = errors;
      let valid3 = false;
      let passing0 = null;
      const _errs10 = errors;
      if (data2 && typeof data2 == 'object' && !Array.isArray(data2)) {
        if (data2.readiness === undefined) {
          const err7 = {
            instancePath: instancePath + '/readiness',
            schemaPath: '#/$defs/BackendReadiness/oneOf/0/required',
            keyword: 'required',
            params: { missingProperty: 'readiness' },
            message: "must have required property '" + 'readiness' + "'",
          };
          if (vErrors === null) {
            vErrors = [err7];
          } else {
            vErrors.push(err7);
          }
          errors++;
        }
        for (const key1 in data2) {
          if (!(key1 === 'readiness')) {
            const err8 = {
              instancePath: instancePath + '/readiness',
              schemaPath: '#/$defs/BackendReadiness/oneOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err8];
            } else {
              vErrors.push(err8);
            }
            errors++;
          }
        }
        if (data2.readiness !== undefined) {
          let data3 = data2.readiness;
          if (typeof data3 !== 'string') {
            const err9 = {
              instancePath: instancePath + '/readiness/readiness',
              schemaPath: '#/$defs/BackendReadiness/oneOf/0/properties/readiness/type',
              keyword: 'type',
              params: { type: 'string' },
              message: 'must be string',
            };
            if (vErrors === null) {
              vErrors = [err9];
            } else {
              vErrors.push(err9);
            }
            errors++;
          }
          if ('ready' !== data3) {
            const err10 = {
              instancePath: instancePath + '/readiness/readiness',
              schemaPath: '#/$defs/BackendReadiness/oneOf/0/properties/readiness/const',
              keyword: 'const',
              params: { allowedValue: 'ready' },
              message: 'must be equal to constant',
            };
            if (vErrors === null) {
              vErrors = [err10];
            } else {
              vErrors.push(err10);
            }
            errors++;
          }
        }
      } else {
        const err11 = {
          instancePath: instancePath + '/readiness',
          schemaPath: '#/$defs/BackendReadiness/oneOf/0/type',
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
      var _valid0 = _errs10 === errors;
      if (_valid0) {
        valid3 = true;
        passing0 = 0;
        var props0 = true;
      }
      const _errs15 = errors;
      if (data2 && typeof data2 == 'object' && !Array.isArray(data2)) {
        if (data2.readiness === undefined) {
          const err12 = {
            instancePath: instancePath + '/readiness',
            schemaPath: '#/$defs/BackendReadiness/oneOf/1/required',
            keyword: 'required',
            params: { missingProperty: 'readiness' },
            message: "must have required property '" + 'readiness' + "'",
          };
          if (vErrors === null) {
            vErrors = [err12];
          } else {
            vErrors.push(err12);
          }
          errors++;
        }
        if (data2.reason === undefined) {
          const err13 = {
            instancePath: instancePath + '/readiness',
            schemaPath: '#/$defs/BackendReadiness/oneOf/1/required',
            keyword: 'required',
            params: { missingProperty: 'reason' },
            message: "must have required property '" + 'reason' + "'",
          };
          if (vErrors === null) {
            vErrors = [err13];
          } else {
            vErrors.push(err13);
          }
          errors++;
        }
        for (const key2 in data2) {
          if (!(key2 === 'readiness' || key2 === 'reason')) {
            const err14 = {
              instancePath: instancePath + '/readiness',
              schemaPath: '#/$defs/BackendReadiness/oneOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key2 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err14];
            } else {
              vErrors.push(err14);
            }
            errors++;
          }
        }
        if (data2.readiness !== undefined) {
          let data4 = data2.readiness;
          if (typeof data4 !== 'string') {
            const err15 = {
              instancePath: instancePath + '/readiness/readiness',
              schemaPath: '#/$defs/BackendReadiness/oneOf/1/properties/readiness/type',
              keyword: 'type',
              params: { type: 'string' },
              message: 'must be string',
            };
            if (vErrors === null) {
              vErrors = [err15];
            } else {
              vErrors.push(err15);
            }
            errors++;
          }
          if ('unavailable' !== data4) {
            const err16 = {
              instancePath: instancePath + '/readiness/readiness',
              schemaPath: '#/$defs/BackendReadiness/oneOf/1/properties/readiness/const',
              keyword: 'const',
              params: { allowedValue: 'unavailable' },
              message: 'must be equal to constant',
            };
            if (vErrors === null) {
              vErrors = [err16];
            } else {
              vErrors.push(err16);
            }
            errors++;
          }
        }
        if (data2.reason !== undefined) {
          if (typeof data2.reason !== 'string') {
            const err17 = {
              instancePath: instancePath + '/readiness/reason',
              schemaPath: '#/$defs/BackendReadiness/oneOf/1/properties/reason/type',
              keyword: 'type',
              params: { type: 'string' },
              message: 'must be string',
            };
            if (vErrors === null) {
              vErrors = [err17];
            } else {
              vErrors.push(err17);
            }
            errors++;
          }
        }
      } else {
        const err18 = {
          instancePath: instancePath + '/readiness',
          schemaPath: '#/$defs/BackendReadiness/oneOf/1/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        };
        if (vErrors === null) {
          vErrors = [err18];
        } else {
          vErrors.push(err18);
        }
        errors++;
      }
      var _valid0 = _errs15 === errors;
      if (_valid0 && valid3) {
        valid3 = false;
        passing0 = [passing0, 1];
      } else {
        if (_valid0) {
          valid3 = true;
          passing0 = 1;
          if (props0 !== true) {
            props0 = true;
          }
        }
        const _errs22 = errors;
        if (data2 && typeof data2 == 'object' && !Array.isArray(data2)) {
          if (data2.readiness === undefined) {
            const err19 = {
              instancePath: instancePath + '/readiness',
              schemaPath: '#/$defs/BackendReadiness/oneOf/2/required',
              keyword: 'required',
              params: { missingProperty: 'readiness' },
              message: "must have required property '" + 'readiness' + "'",
            };
            if (vErrors === null) {
              vErrors = [err19];
            } else {
              vErrors.push(err19);
            }
            errors++;
          }
          if (data2.reason === undefined) {
            const err20 = {
              instancePath: instancePath + '/readiness',
              schemaPath: '#/$defs/BackendReadiness/oneOf/2/required',
              keyword: 'required',
              params: { missingProperty: 'reason' },
              message: "must have required property '" + 'reason' + "'",
            };
            if (vErrors === null) {
              vErrors = [err20];
            } else {
              vErrors.push(err20);
            }
            errors++;
          }
          for (const key3 in data2) {
            if (!(key3 === 'readiness' || key3 === 'reason')) {
              const err21 = {
                instancePath: instancePath + '/readiness',
                schemaPath: '#/$defs/BackendReadiness/oneOf/2/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key3 },
                message: 'must NOT have additional properties',
              };
              if (vErrors === null) {
                vErrors = [err21];
              } else {
                vErrors.push(err21);
              }
              errors++;
            }
          }
          if (data2.readiness !== undefined) {
            let data6 = data2.readiness;
            if (typeof data6 !== 'string') {
              const err22 = {
                instancePath: instancePath + '/readiness/readiness',
                schemaPath: '#/$defs/BackendReadiness/oneOf/2/properties/readiness/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err22];
              } else {
                vErrors.push(err22);
              }
              errors++;
            }
            if ('setup_required' !== data6) {
              const err23 = {
                instancePath: instancePath + '/readiness/readiness',
                schemaPath: '#/$defs/BackendReadiness/oneOf/2/properties/readiness/const',
                keyword: 'const',
                params: { allowedValue: 'setup_required' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err23];
              } else {
                vErrors.push(err23);
              }
              errors++;
            }
          }
          if (data2.reason !== undefined) {
            if (typeof data2.reason !== 'string') {
              const err24 = {
                instancePath: instancePath + '/readiness/reason',
                schemaPath: '#/$defs/BackendReadiness/oneOf/2/properties/reason/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err24];
              } else {
                vErrors.push(err24);
              }
              errors++;
            }
          }
        } else {
          const err25 = {
            instancePath: instancePath + '/readiness',
            schemaPath: '#/$defs/BackendReadiness/oneOf/2/type',
            keyword: 'type',
            params: { type: 'object' },
            message: 'must be object',
          };
          if (vErrors === null) {
            vErrors = [err25];
          } else {
            vErrors.push(err25);
          }
          errors++;
        }
        var _valid0 = _errs22 === errors;
        if (_valid0 && valid3) {
          valid3 = false;
          passing0 = [passing0, 2];
        } else {
          if (_valid0) {
            valid3 = true;
            passing0 = 2;
            if (props0 !== true) {
              props0 = true;
            }
          }
          const _errs29 = errors;
          if (data2 && typeof data2 == 'object' && !Array.isArray(data2)) {
            if (data2.readiness === undefined) {
              const err26 = {
                instancePath: instancePath + '/readiness',
                schemaPath: '#/$defs/BackendReadiness/oneOf/3/required',
                keyword: 'required',
                params: { missingProperty: 'readiness' },
                message: "must have required property '" + 'readiness' + "'",
              };
              if (vErrors === null) {
                vErrors = [err26];
              } else {
                vErrors.push(err26);
              }
              errors++;
            }
            if (data2.reason === undefined) {
              const err27 = {
                instancePath: instancePath + '/readiness',
                schemaPath: '#/$defs/BackendReadiness/oneOf/3/required',
                keyword: 'required',
                params: { missingProperty: 'reason' },
                message: "must have required property '" + 'reason' + "'",
              };
              if (vErrors === null) {
                vErrors = [err27];
              } else {
                vErrors.push(err27);
              }
              errors++;
            }
            for (const key4 in data2) {
              if (!(key4 === 'readiness' || key4 === 'reason')) {
                const err28 = {
                  instancePath: instancePath + '/readiness',
                  schemaPath: '#/$defs/BackendReadiness/oneOf/3/additionalProperties',
                  keyword: 'additionalProperties',
                  params: { additionalProperty: key4 },
                  message: 'must NOT have additional properties',
                };
                if (vErrors === null) {
                  vErrors = [err28];
                } else {
                  vErrors.push(err28);
                }
                errors++;
              }
            }
            if (data2.readiness !== undefined) {
              let data8 = data2.readiness;
              if (typeof data8 !== 'string') {
                const err29 = {
                  instancePath: instancePath + '/readiness/readiness',
                  schemaPath: '#/$defs/BackendReadiness/oneOf/3/properties/readiness/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                };
                if (vErrors === null) {
                  vErrors = [err29];
                } else {
                  vErrors.push(err29);
                }
                errors++;
              }
              if ('repair_required' !== data8) {
                const err30 = {
                  instancePath: instancePath + '/readiness/readiness',
                  schemaPath: '#/$defs/BackendReadiness/oneOf/3/properties/readiness/const',
                  keyword: 'const',
                  params: { allowedValue: 'repair_required' },
                  message: 'must be equal to constant',
                };
                if (vErrors === null) {
                  vErrors = [err30];
                } else {
                  vErrors.push(err30);
                }
                errors++;
              }
            }
            if (data2.reason !== undefined) {
              if (typeof data2.reason !== 'string') {
                const err31 = {
                  instancePath: instancePath + '/readiness/reason',
                  schemaPath: '#/$defs/BackendReadiness/oneOf/3/properties/reason/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                };
                if (vErrors === null) {
                  vErrors = [err31];
                } else {
                  vErrors.push(err31);
                }
                errors++;
              }
            }
          } else {
            const err32 = {
              instancePath: instancePath + '/readiness',
              schemaPath: '#/$defs/BackendReadiness/oneOf/3/type',
              keyword: 'type',
              params: { type: 'object' },
              message: 'must be object',
            };
            if (vErrors === null) {
              vErrors = [err32];
            } else {
              vErrors.push(err32);
            }
            errors++;
          }
          var _valid0 = _errs29 === errors;
          if (_valid0 && valid3) {
            valid3 = false;
            passing0 = [passing0, 3];
          } else {
            if (_valid0) {
              valid3 = true;
              passing0 = 3;
              if (props0 !== true) {
                props0 = true;
              }
            }
          }
        }
      }
      if (!valid3) {
        const err33 = {
          instancePath: instancePath + '/readiness',
          schemaPath: '#/$defs/BackendReadiness/oneOf',
          keyword: 'oneOf',
          params: { passingSchemas: passing0 },
          message: 'must match exactly one schema in oneOf',
        };
        if (vErrors === null) {
          vErrors = [err33];
        } else {
          vErrors.push(err33);
        }
        errors++;
      } else {
        errors = _errs9;
        if (vErrors !== null) {
          if (_errs9) {
            vErrors.length = _errs9;
          } else {
            vErrors = null;
          }
        }
      }
    }
  } else {
    const err34 = {
      instancePath,
      schemaPath: '#/type',
      keyword: 'type',
      params: { type: 'object' },
      message: 'must be object',
    };
    if (vErrors === null) {
      vErrors = [err34];
    } else {
      vErrors.push(err34);
    }
    errors++;
  }
  validate22.errors = vErrors;
  return errors === 0;
}
validate22.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema36 = {
  additionalProperties: false,
  properties: {
    capability: { type: 'string' },
    readiness: { $ref: '#/$defs/CapabilityReadiness' },
    reason_code: { type: 'string' },
  },
  required: ['capability', 'readiness', 'reason_code'],
  type: 'object',
};
const schema37 = { enum: ['ready', 'unavailable'], type: 'string' };
function validate24(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate24.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (data && typeof data == 'object' && !Array.isArray(data)) {
    if (data.capability === undefined) {
      const err0 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'capability' },
        message: "must have required property '" + 'capability' + "'",
      };
      if (vErrors === null) {
        vErrors = [err0];
      } else {
        vErrors.push(err0);
      }
      errors++;
    }
    if (data.readiness === undefined) {
      const err1 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'readiness' },
        message: "must have required property '" + 'readiness' + "'",
      };
      if (vErrors === null) {
        vErrors = [err1];
      } else {
        vErrors.push(err1);
      }
      errors++;
    }
    if (data.reason_code === undefined) {
      const err2 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'reason_code' },
        message: "must have required property '" + 'reason_code' + "'",
      };
      if (vErrors === null) {
        vErrors = [err2];
      } else {
        vErrors.push(err2);
      }
      errors++;
    }
    for (const key0 in data) {
      if (!(key0 === 'capability' || key0 === 'readiness' || key0 === 'reason_code')) {
        const err3 = {
          instancePath,
          schemaPath: '#/additionalProperties',
          keyword: 'additionalProperties',
          params: { additionalProperty: key0 },
          message: 'must NOT have additional properties',
        };
        if (vErrors === null) {
          vErrors = [err3];
        } else {
          vErrors.push(err3);
        }
        errors++;
      }
    }
    if (data.capability !== undefined) {
      if (typeof data.capability !== 'string') {
        const err4 = {
          instancePath: instancePath + '/capability',
          schemaPath: '#/properties/capability/type',
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
    }
    if (data.readiness !== undefined) {
      let data1 = data.readiness;
      if (typeof data1 !== 'string') {
        const err5 = {
          instancePath: instancePath + '/readiness',
          schemaPath: '#/$defs/CapabilityReadiness/type',
          keyword: 'type',
          params: { type: 'string' },
          message: 'must be string',
        };
        if (vErrors === null) {
          vErrors = [err5];
        } else {
          vErrors.push(err5);
        }
        errors++;
      }
      if (!(data1 === 'ready' || data1 === 'unavailable')) {
        const err6 = {
          instancePath: instancePath + '/readiness',
          schemaPath: '#/$defs/CapabilityReadiness/enum',
          keyword: 'enum',
          params: { allowedValues: schema37.enum },
          message: 'must be equal to one of the allowed values',
        };
        if (vErrors === null) {
          vErrors = [err6];
        } else {
          vErrors.push(err6);
        }
        errors++;
      }
    }
    if (data.reason_code !== undefined) {
      if (typeof data.reason_code !== 'string') {
        const err7 = {
          instancePath: instancePath + '/reason_code',
          schemaPath: '#/properties/reason_code/type',
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
    }
  } else {
    const err8 = {
      instancePath,
      schemaPath: '#/type',
      keyword: 'type',
      params: { type: 'object' },
      message: 'must be object',
    };
    if (vErrors === null) {
      vErrors = [err8];
    } else {
      vErrors.push(err8);
    }
    errors++;
  }
  validate24.errors = vErrors;
  return errors === 0;
}
validate24.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema39 = {
  additionalProperties: false,
  properties: {
    extension: { type: 'string' },
    readiness: { $ref: '#/$defs/ExtensionReadiness' },
    reason_code: { type: 'string' },
  },
  required: ['extension', 'readiness', 'reason_code'],
  type: 'object',
};
const schema40 = { enum: ['disabled', 'ready', 'degraded'], type: 'string' };
function validate26(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate26.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (data && typeof data == 'object' && !Array.isArray(data)) {
    if (data.extension === undefined) {
      const err0 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'extension' },
        message: "must have required property '" + 'extension' + "'",
      };
      if (vErrors === null) {
        vErrors = [err0];
      } else {
        vErrors.push(err0);
      }
      errors++;
    }
    if (data.readiness === undefined) {
      const err1 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'readiness' },
        message: "must have required property '" + 'readiness' + "'",
      };
      if (vErrors === null) {
        vErrors = [err1];
      } else {
        vErrors.push(err1);
      }
      errors++;
    }
    if (data.reason_code === undefined) {
      const err2 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'reason_code' },
        message: "must have required property '" + 'reason_code' + "'",
      };
      if (vErrors === null) {
        vErrors = [err2];
      } else {
        vErrors.push(err2);
      }
      errors++;
    }
    for (const key0 in data) {
      if (!(key0 === 'extension' || key0 === 'readiness' || key0 === 'reason_code')) {
        const err3 = {
          instancePath,
          schemaPath: '#/additionalProperties',
          keyword: 'additionalProperties',
          params: { additionalProperty: key0 },
          message: 'must NOT have additional properties',
        };
        if (vErrors === null) {
          vErrors = [err3];
        } else {
          vErrors.push(err3);
        }
        errors++;
      }
    }
    if (data.extension !== undefined) {
      if (typeof data.extension !== 'string') {
        const err4 = {
          instancePath: instancePath + '/extension',
          schemaPath: '#/properties/extension/type',
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
    }
    if (data.readiness !== undefined) {
      let data1 = data.readiness;
      if (typeof data1 !== 'string') {
        const err5 = {
          instancePath: instancePath + '/readiness',
          schemaPath: '#/$defs/ExtensionReadiness/type',
          keyword: 'type',
          params: { type: 'string' },
          message: 'must be string',
        };
        if (vErrors === null) {
          vErrors = [err5];
        } else {
          vErrors.push(err5);
        }
        errors++;
      }
      if (!(data1 === 'disabled' || data1 === 'ready' || data1 === 'degraded')) {
        const err6 = {
          instancePath: instancePath + '/readiness',
          schemaPath: '#/$defs/ExtensionReadiness/enum',
          keyword: 'enum',
          params: { allowedValues: schema40.enum },
          message: 'must be equal to one of the allowed values',
        };
        if (vErrors === null) {
          vErrors = [err6];
        } else {
          vErrors.push(err6);
        }
        errors++;
      }
    }
    if (data.reason_code !== undefined) {
      if (typeof data.reason_code !== 'string') {
        const err7 = {
          instancePath: instancePath + '/reason_code',
          schemaPath: '#/properties/reason_code/type',
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
    }
  } else {
    const err8 = {
      instancePath,
      schemaPath: '#/type',
      keyword: 'type',
      params: { type: 'object' },
      message: 'must be object',
    };
    if (vErrors === null) {
      vErrors = [err8];
    } else {
      vErrors.push(err8);
    }
    errors++;
  }
  validate26.errors = vErrors;
  return errors === 0;
}
validate26.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate21(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate21.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (data && typeof data == 'object' && !Array.isArray(data)) {
    if (data.schema_version === undefined) {
      const err0 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'schema_version' },
        message: "must have required property '" + 'schema_version' + "'",
      };
      if (vErrors === null) {
        vErrors = [err0];
      } else {
        vErrors.push(err0);
      }
      errors++;
    }
    if (data.revision === undefined) {
      const err1 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'revision' },
        message: "must have required property '" + 'revision' + "'",
      };
      if (vErrors === null) {
        vErrors = [err1];
      } else {
        vErrors.push(err1);
      }
      errors++;
    }
    if (data.core_health === undefined) {
      const err2 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'core_health' },
        message: "must have required property '" + 'core_health' + "'",
      };
      if (vErrors === null) {
        vErrors = [err2];
      } else {
        vErrors.push(err2);
      }
      errors++;
    }
    if (data.capabilities === undefined) {
      const err3 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'capabilities' },
        message: "must have required property '" + 'capabilities' + "'",
      };
      if (vErrors === null) {
        vErrors = [err3];
      } else {
        vErrors.push(err3);
      }
      errors++;
    }
    if (data.backends === undefined) {
      const err4 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'backends' },
        message: "must have required property '" + 'backends' + "'",
      };
      if (vErrors === null) {
        vErrors = [err4];
      } else {
        vErrors.push(err4);
      }
      errors++;
    }
    if (data.extensions === undefined) {
      const err5 = {
        instancePath,
        schemaPath: '#/required',
        keyword: 'required',
        params: { missingProperty: 'extensions' },
        message: "must have required property '" + 'extensions' + "'",
      };
      if (vErrors === null) {
        vErrors = [err5];
      } else {
        vErrors.push(err5);
      }
      errors++;
    }
    for (const key0 in data) {
      if (!(
        key0 === 'backends' ||
        key0 === 'capabilities' ||
        key0 === 'core_health' ||
        key0 === 'extensions' ||
        key0 === 'revision' ||
        key0 === 'schema_version'
      )) {
        const err6 = {
          instancePath,
          schemaPath: '#/additionalProperties',
          keyword: 'additionalProperties',
          params: { additionalProperty: key0 },
          message: 'must NOT have additional properties',
        };
        if (vErrors === null) {
          vErrors = [err6];
        } else {
          vErrors.push(err6);
        }
        errors++;
      }
    }
    if (data.backends !== undefined) {
      let data0 = data.backends;
      if (Array.isArray(data0)) {
        const len0 = data0.length;
        for (let i0 = 0; i0 < len0; i0++) {
          if (
            !validate22(data0[i0], {
              instancePath: instancePath + '/backends/' + i0,
              parentData: data0,
              parentDataProperty: i0,
              rootData,
              dynamicAnchors,
            })
          ) {
            vErrors = vErrors === null ? validate22.errors : vErrors.concat(validate22.errors);
            errors = vErrors.length;
          }
        }
      } else {
        const err7 = {
          instancePath: instancePath + '/backends',
          schemaPath: '#/properties/backends/type',
          keyword: 'type',
          params: { type: 'array' },
          message: 'must be array',
        };
        if (vErrors === null) {
          vErrors = [err7];
        } else {
          vErrors.push(err7);
        }
        errors++;
      }
    }
    if (data.capabilities !== undefined) {
      let data2 = data.capabilities;
      if (Array.isArray(data2)) {
        const len1 = data2.length;
        for (let i1 = 0; i1 < len1; i1++) {
          if (
            !validate24(data2[i1], {
              instancePath: instancePath + '/capabilities/' + i1,
              parentData: data2,
              parentDataProperty: i1,
              rootData,
              dynamicAnchors,
            })
          ) {
            vErrors = vErrors === null ? validate24.errors : vErrors.concat(validate24.errors);
            errors = vErrors.length;
          }
        }
      } else {
        const err8 = {
          instancePath: instancePath + '/capabilities',
          schemaPath: '#/properties/capabilities/type',
          keyword: 'type',
          params: { type: 'array' },
          message: 'must be array',
        };
        if (vErrors === null) {
          vErrors = [err8];
        } else {
          vErrors.push(err8);
        }
        errors++;
      }
    }
    if (data.core_health !== undefined) {
      let data4 = data.core_health;
      if (typeof data4 !== 'string') {
        const err9 = {
          instancePath: instancePath + '/core_health',
          schemaPath: '#/$defs/CoreHealth/type',
          keyword: 'type',
          params: { type: 'string' },
          message: 'must be string',
        };
        if (vErrors === null) {
          vErrors = [err9];
        } else {
          vErrors.push(err9);
        }
        errors++;
      }
      if (!(data4 === 'starting' || data4 === 'ready' || data4 === 'recovery_required')) {
        const err10 = {
          instancePath: instancePath + '/core_health',
          schemaPath: '#/$defs/CoreHealth/enum',
          keyword: 'enum',
          params: { allowedValues: schema38.enum },
          message: 'must be equal to one of the allowed values',
        };
        if (vErrors === null) {
          vErrors = [err10];
        } else {
          vErrors.push(err10);
        }
        errors++;
      }
    }
    if (data.extensions !== undefined) {
      let data5 = data.extensions;
      if (Array.isArray(data5)) {
        const len2 = data5.length;
        for (let i2 = 0; i2 < len2; i2++) {
          if (
            !validate26(data5[i2], {
              instancePath: instancePath + '/extensions/' + i2,
              parentData: data5,
              parentDataProperty: i2,
              rootData,
              dynamicAnchors,
            })
          ) {
            vErrors = vErrors === null ? validate26.errors : vErrors.concat(validate26.errors);
            errors = vErrors.length;
          }
        }
      } else {
        const err11 = {
          instancePath: instancePath + '/extensions',
          schemaPath: '#/properties/extensions/type',
          keyword: 'type',
          params: { type: 'array' },
          message: 'must be array',
        };
        if (vErrors === null) {
          vErrors = [err11];
        } else {
          vErrors.push(err11);
        }
        errors++;
      }
    }
    if (data.revision !== undefined) {
      if (typeof data.revision !== 'string') {
        const err12 = {
          instancePath: instancePath + '/revision',
          schemaPath: '#/$defs/Revision/type',
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
    }
    if (data.schema_version !== undefined) {
      let data8 = data.schema_version;
      if (!(typeof data8 == 'number' && !(data8 % 1) && !isNaN(data8) && isFinite(data8))) {
        const err13 = {
          instancePath: instancePath + '/schema_version',
          schemaPath: '#/$defs/ToolWireVersionV1/type',
          keyword: 'type',
          params: { type: 'integer' },
          message: 'must be integer',
        };
        if (vErrors === null) {
          vErrors = [err13];
        } else {
          vErrors.push(err13);
        }
        errors++;
      }
      if (1 !== data8) {
        const err14 = {
          instancePath: instancePath + '/schema_version',
          schemaPath: '#/$defs/ToolWireVersionV1/const',
          keyword: 'const',
          params: { allowedValue: 1 },
          message: 'must be equal to constant',
        };
        if (vErrors === null) {
          vErrors = [err14];
        } else {
          vErrors.push(err14);
        }
        errors++;
      }
    }
  } else {
    const err15 = {
      instancePath,
      schemaPath: '#/type',
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
  validate21.errors = vErrors;
  return errors === 0;
}
validate21.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate20(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate20.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (
    !validate21(data, { instancePath, parentData, parentDataProperty, rootData, dynamicAnchors })
  ) {
    vErrors = vErrors === null ? validate21.errors : vErrors.concat(validate21.errors);
    errors = vErrors.length;
  }
  validate20.errors = vErrors;
  return errors === 0;
}
validate20.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
