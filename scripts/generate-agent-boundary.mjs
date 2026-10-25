import Ajv2020 from '../packages/contracts/node_modules/ajv/dist/2020.js';
import standaloneCode from '../packages/contracts/node_modules/ajv/dist/standalone/index.js';
import equalRuntime from '../packages/contracts/node_modules/ajv/dist/runtime/equal.js';
import lengthRuntime from '../packages/contracts/node_modules/ajv/dist/runtime/ucs2length.js';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { format } from 'prettier';

/** Generate the exact Rust schema closure; no hand-written Agent DTO guard. */
export async function generateAgentBoundary(
  aggregate,
  routes,
  directory,
  idTypes,
  owner = 'agent',
) {
  const selected = [
    ...new Map(
      routes
        .filter(
          (r) =>
            (r.surface === 'main' || (owner === 'memory' && r.surface === 'core')) &&
            (r.owner === owner ||
              (owner === 'agent' &&
                (r.method.startsWith('session.') || r.method === 'workspace.select_directory'))),
        )
        .map((route) => [route.method, route]),
    ).values(),
  ];
  const defs = {};
  function visit(value) {
    if (!value || typeof value !== 'object') return;
    for (const [key, item] of Object.entries(value)) {
      if (key === '$ref' && item.startsWith('#/$defs/')) {
        const name = item.slice(8);
        if (!defs[name]) {
          if (!aggregate.$defs[name]) throw Error(`missing Agent schema: ${name}`);
          defs[name] = structuredClone(aggregate.$defs[name]);
          visit(defs[name]);
        }
      } else visit(item);
    }
    if (value.format === 'uri' && value.type === 'string' && typeof value.pattern === 'string') {
      // A Rust-validated string whose pattern already bounds it; AJV standalone has
      // no format plugin, and the Rust parser remains the authority.
      delete value.format;
      return;
    }
    if (typeof value.format === 'string') {
      // JSON/JS cannot represent integers outside the safe integer domain.
      // Replace numeric format extensions with equivalent generated bounds;
      // do not silently ignore a newly introduced or string format.
      const limits = {
        uint8: 255,
        uint16: 65535,
        uint32: 4294967295,
        uint64: Number.MAX_SAFE_INTEGER,
      };
      if (
        (value.format === 'double' || value.format === 'float') &&
        (value.type === 'number' ||
          (Array.isArray(value.type) &&
            value.type.includes('number') &&
            value.type.every((t) => t === 'number' || t === 'null')))
      ) {
        delete value.format;
        return;
      }
      const integer =
        value.type === 'integer' ||
        (Array.isArray(value.type) &&
          value.type.includes('integer') &&
          value.type.every((t) => t === 'integer' || t === 'null'));
      // Signed formats keep their own lower bound instead of the unsigned zero.
      const signed = { int8: 127, int16: 32767, int32: 2147483647, int64: Number.MAX_SAFE_INTEGER };
      if (integer && Object.hasOwn(signed, value.format)) {
        const low = value.format === 'int64' ? Number.MIN_SAFE_INTEGER : -signed[value.format] - 1;
        value.minimum = Math.max(value.minimum ?? low, low);
        value.maximum = Math.min(value.maximum ?? signed[value.format], signed[value.format]);
        delete value.format;
        return;
      }
      if (!integer || !Object.hasOwn(limits, value.format))
        throw Error(`unhandled Agent schema format: ${value.format}`);
      value.minimum = Math.max(value.minimum ?? 0, 0);
      value.maximum = Math.min(value.maximum ?? limits[value.format], limits[value.format]);
      delete value.format;
    }
  }
  const envelope = (field) => ({
    $schema: aggregate.$schema,
    anyOf: selected.map((r) => ({
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: { command: { const: r.method }, value: { $ref: `#/$defs/${r[field]}` } },
    })),
    $defs: defs,
  });
  for (const route of selected) {
    visit({ $ref: `#/$defs/${route.request_type}` });
    visit({ $ref: `#/$defs/${route.response_type}` });
  }
  // Session creation and coordinated mode changes emit this existing Security
  // notification. Its payload must evolve with the same Rust source of truth.
  if (owner === 'agent') {
    visit({ $ref: '#/$defs/SessionPermissionProjection' });
    visit({ $ref: '#/$defs/AddManualModelInput' });
  }
  const gitWrites = {
    'git.commit.input': 'GitCommitInput',
    'git.branch.input': 'GitBranchInput',
    'git.commit.result': 'GitCommitResultV1',
    'git.branch.result': 'GitBranchResultV1',
    'git.sync.input': 'GitSyncInput',
    'git.push.input': 'GitPushInput',
    'git.sync.result': 'GitRemoteResultV1',
    'git.push.result': 'GitRemoteResultV1',
  };
  if (owner === 'git')
    Object.values(gitWrites).forEach((name) => visit({ $ref: `#/$defs/${name}` }));
  // Schemars' legacy ID wrappers expose only String. The Rust registry supplies
  // their exact type/prefix relation without a second hand-written DTO table.
  for (const { type_name: name, prefix } of idTypes.entries) {
    if (!name || !/^[a-z]{3}$/u.test(prefix)) throw Error('Invalid Rust ID registry');
    if (defs[name])
      defs[name] = {
        ...defs[name],
        minLength: 30,
        maxLength: 30,
        pattern: `^${prefix}_[0-7][0-9A-HJKMNP-TV-Z]{25}$`,
      };
  }
  const ajv = new Ajv2020({
    strict: true,
    allErrors: false,
    inlineRefs: false,
    code: { source: true, esm: true },
  });
  ajv.addSchema(envelope('request_type'), 'aura-agent-requests');
  ajv.addSchema(envelope('response_type'), 'aura-agent-responses');
  if (owner === 'git') {
    ajv.addSchema(
      {
        $schema: aggregate.$schema,
        anyOf: Object.entries(gitWrites).map(([command, type]) => ({
          type: 'object',
          additionalProperties: false,
          required: ['command', 'value'],
          properties: { command: { const: command }, value: { $ref: `#/$defs/${type}` } },
        })),
        $defs: defs,
      },
      'aura-git-write-contracts',
    );
  }
  if (owner === 'agent') {
    ajv.addSchema(
      { $schema: aggregate.$schema, $ref: '#/$defs/SessionPermissionProjection', $defs: defs },
      'aura-session-permission-projection',
    );
    ajv.addSchema(
      { $schema: aggregate.$schema, $ref: '#/$defs/AddManualModelInput', $defs: defs },
      'aura-manual-model-profile-input',
    );
  }
  let code = standaloneCode(ajv, {
    validateAgentRequest: 'aura-agent-requests',
    validateAgentResponse: 'aura-agent-responses',
    ...(owner === 'git' ? { validateGitWriteContract: 'aura-git-write-contracts' } : {}),
    ...(owner === 'agent'
      ? {
          validateSessionPermissionProjection: 'aura-session-permission-projection',
          validateManualModelProfileInput: 'aura-manual-model-profile-input',
        }
      : {}),
  });
  if (owner === 'git') {
    code = code
      .replaceAll('validateAgentRequest', 'validateGitRequest')
      .replaceAll('validateAgentResponse', 'validateGitResponse');
  }
  if (owner === 'package') {
    code = code
      .replaceAll('validateAgentRequest', 'validatePackageRequest')
      .replaceAll('validateAgentResponse', 'validatePackageResponse');
  }
  if (owner === 'memory') {
    code = code
      .replaceAll('validateAgentRequest', 'validateMemoryRequest')
      .replaceAll('validateAgentResponse', 'validateMemoryResponse');
  }
  // AJV emits CommonJS helper references even for ESM standalone. Inline the
  // installed library's exact implementations so sandboxed preload needs no
  // require/eval or extra dependency resolver. Reject unfamiliar helpers.
  for (const [name, implementation] of [
    ['ucs2length', lengthRuntime.default],
    ['equal', equalRuntime.default],
  ]) {
    code = code.replaceAll(
      `require("ajv/dist/runtime/${name}").default`,
      `(${implementation.toString()})`,
    );
  }
  if (/\brequire\s*\(/u.test(code)) throw Error('Unhandled Agent validator runtime dependency');
  const license = await readFile(
    new URL('../packages/contracts/node_modules/ajv/LICENSE', import.meta.url),
    'utf8',
  );
  await writeFile(
    resolve(directory, `${owner}-validator.ts`),
    await format(
      '// @ts-nocheck\n/* eslint-disable */\n/** Generated from Rust route/schema facts using AJV standalone. Do not edit. */\n' +
        `/* AJV runtime helpers, and fast-deep-equal (Copyright (c) 2017 Evgeny Poberezkin).\n${license}*/\n` +
        code,
      { parser: 'typescript', singleQuote: true, printWidth: 100, endOfLine: 'lf' },
    ),
  );
}
