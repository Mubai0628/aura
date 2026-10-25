import Ajv2020 from '../packages/contracts/node_modules/ajv/dist/2020.js';
import standaloneCode from '../packages/contracts/node_modules/ajv/dist/standalone/index.js';
import equalRuntime from '../packages/contracts/node_modules/ajv/dist/runtime/equal.js';
import lengthRuntime from '../packages/contracts/node_modules/ajv/dist/runtime/ucs2length.js';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { format } from 'prettier';

/** Export the exact closure of the recovery routes, with no duplicated DTO fields. */
export async function generateRecoveryBoundary(aggregate, routes, directory) {
  return generateBoundary(
    aggregate,
    routes.filter((r) => r.surface === 'main' && r.method.startsWith('config.recovery.')),
    directory,
    'Recovery',
    'recovery',
    3,
  );
}

export async function generateToolCredentialBoundary(aggregate, routes, directory) {
  return generateBoundary(
    aggregate,
    routes.filter(
      (r) =>
        r.surface === 'main' &&
        r.method.startsWith('tools.credential.') &&
        r.method !== 'tools.credential.candidates',
    ),
    directory,
    'ToolCredential',
    'tool-credential',
    7,
  );
}

async function generateBoundary(aggregate, selected, directory, label, slug, count) {
  if (selected.length !== count)
    throw Error(`${label} boundary must register ${count} Main routes`);
  const defs = {};
  function visit(value) {
    if (!value || typeof value !== 'object') return;
    for (const [key, item] of Object.entries(value)) {
      if (key === '$ref' && item.startsWith('#/$defs/')) {
        const name = item.slice(8);
        if (!defs[name]) {
          if (!aggregate.$defs[name]) throw Error(`missing recovery schema: ${name}`);
          defs[name] = structuredClone(aggregate.$defs[name]);
          visit(defs[name]);
        }
      } else visit(item);
    }
    if (value.format !== undefined) {
      const limits = {
        uint8: 255,
        uint16: 65535,
        uint32: 4294967295,
        uint64: Number.MAX_SAFE_INTEGER,
      };
      const integer =
        value.type === 'integer' ||
        (Array.isArray(value.type) &&
          value.type.includes('integer') &&
          value.type.every((t) => t === 'integer' || t === 'null'));
      if (!integer || !Object.hasOwn(limits, value.format))
        throw Error(`unhandled recovery schema format: ${value.format}`);
      value.minimum = Math.max(value.minimum ?? 0, 0);
      value.maximum = Math.min(value.maximum ?? limits[value.format], limits[value.format]);
      delete value.format;
    }
  }
  const shape = (type) =>
    type === 'Record<string, never>'
      ? { type: 'object', additionalProperties: false }
      : { $ref: `#/$defs/${type}` };
  for (const route of selected) {
    visit(shape(route.request_type));
    visit(shape(route.response_type));
  }
  const envelope = (field) => ({
    $schema: aggregate.$schema,
    anyOf: selected.map((route) => ({
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: { command: { const: route.method }, value: shape(route[field]) },
    })),
    $defs: defs,
  });
  const ajv = new Ajv2020({
    strict: true,
    allErrors: false,
    inlineRefs: false,
    code: { source: true, esm: true },
  });
  ajv.addSchema(envelope('request_type'), `aura-${slug}-request`);
  ajv.addSchema(envelope('response_type'), `aura-${slug}-response`);
  let code = standaloneCode(ajv, {
    [`validate${label}Request`]: `aura-${slug}-request`,
    [`validate${label}Response`]: `aura-${slug}-response`,
  });
  for (const [name, implementation] of [
    ['ucs2length', lengthRuntime.default],
    ['equal', equalRuntime.default],
  ]) {
    code = code.replaceAll(
      `require("ajv/dist/runtime/${name}").default`,
      `(${implementation.toString()})`,
    );
  }
  if (/\brequire\s*\(/u.test(code)) throw Error('Unexpected typed boundary runtime dependency');
  const license = await readFile(
    new URL('../packages/contracts/node_modules/ajv/LICENSE', import.meta.url),
    'utf8',
  );
  await writeFile(
    resolve(directory, `${slug}-validator.ts`),
    await format(
      '// @ts-nocheck\n/* eslint-disable */\n/** Generated from Rust routes and schemas. Do not edit. */\n/* AJV license: ' +
        license.replaceAll('*/', '* /') +
        ' */\n' +
        code,
      { parser: 'typescript', singleQuote: true, printWidth: 100, endOfLine: 'lf' },
    ),
  );
}
