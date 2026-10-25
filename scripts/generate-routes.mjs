import Ajv2020 from '../packages/contracts/node_modules/ajv/dist/2020.js';
import standaloneCode from '../packages/contracts/node_modules/ajv/dist/standalone/index.js';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { format } from 'prettier';
import { generateAgentBoundary } from './generate-agent-boundary.mjs';
import {
  generateRecoveryBoundary,
  generateToolCredentialBoundary,
} from './generate-recovery-boundary.mjs';

const args = new Map();
for (let i = 2; i < process.argv.length; i += 2) args.set(process.argv[i], process.argv[i + 1]);
const input = resolve(args.get('--input'));
const output = resolve(args.get('--output'));
const routes = JSON.parse(await readFile(input, 'utf8'));
const keys = new Set();
for (const route of routes) {
  const key = `${route.surface}:${route.method}`;
  if (keys.has(key) || route.test_only)
    throw Error(`duplicate or test-only production route: ${key}`);
  keys.add(key);
}
const groups = [];
for (const [phase, owner] of [
  [3, 'workspace'],
  [4, 'security'],
  [5, 'model'],
  [7, 'tool'],
]) {
  for (const surface of ['Main', 'Core']) {
    const selected = routes.filter(
      (r) =>
        r.owner === owner &&
        !(r.method.startsWith('tools.credential.') && r.method !== 'tools.credential.candidates') &&
        r.surface === surface.toLowerCase() &&
        !r.method.startsWith('ui.') &&
        (phase !== 7 || r.idempotency !== 'read_only'),
    );
    groups.push([`P0${phase}${surface}`, selected]);
  }
}
groups.push([
  'P07Read',
  routes.filter((r) => r.surface === 'main' && r.method === 'tools.credential.candidates'),
]);
for (const surface of ['Main', 'Core']) {
  groups.push([
    `Git${surface}`,
    routes.filter((r) => r.surface === surface.toLowerCase() && r.owner === 'git'),
  ]);
  groups.push([
    `Agent${surface}`,
    routes.filter((r) => r.surface === surface.toLowerCase() && r.owner === 'agent'),
  ]);
  groups.push([
    `Package${surface}`,
    routes.filter((r) => r.surface === surface.toLowerCase() && r.owner === 'package'),
  ]);
  groups.push([
    `Memory${surface}`,
    routes.filter((r) => r.surface === surface.toLowerCase() && r.owner === 'memory'),
  ]);
}
const custom = {
  RendererLifecycleAck: 'p03-validation',
  RendererIssueDiscardConfirmation: 'p03-validation',
  RendererResolveWorkspaceLifecycle: 'p03-validation',
  RendererAttachExternalReadOnlyInput: 'p04-validation',
  RendererToolDraftImportArchiveRequestV1: 'p07-validation',
  RendererToolDraftImportDirectoryRequestV1: 'p07-validation',
};
for (const surface of ['Main', 'Core']) {
  groups.push([
    `ConfigRecovery${surface}`,
    routes.filter(
      (r) => r.surface === surface.toLowerCase() && r.method.startsWith('config.recovery.'),
    ),
  ]);
}
for (const surface of ['Main', 'Core']) {
  groups.push([
    `ToolCredential${surface}`,
    routes.filter(
      (r) =>
        r.surface === surface.toLowerCase() &&
        r.method.startsWith('tools.credential.') &&
        r.method !== 'tools.credential.candidates',
    ),
  ]);
}
groups.push([
  'RuntimeMain',
  routes.filter((r) => r.surface === 'main' && r.method === 'runtime.readiness'),
]);
const types = new Set(
  groups.flatMap(([, rs]) =>
    rs
      .flatMap((r) => [r.request_type, r.response_type])
      .flatMap((t) => t.match(/\b[A-Z]\w*/g) ?? []),
  ),
);
types.delete('Record');
let source = '/** Generated from the Rust Route Registry. Do not edit. */\n';
source += `import type {${[...types]
  .filter((t) => !custom[t])
  .sort()
  .join(',')}} from '@aura/contracts/generated';\n`;
for (const [name, path] of Object.entries(custom))
  if (types.has(name)) source += `import type {${name}} from '../${path}.js';\n`;
for (const [name, rs] of groups) {
  for (const [suffix, field] of [
    ['Params', 'request_type'],
    ['Result', 'response_type'],
  ])
    source += `export type ${name}${suffix} = {\n${rs.map((r) => `${JSON.stringify(r.method)}: ${r[field]};`).join('\n')}\n};\n`;
  source += `export type ${name}Command = keyof ${name}Params;\n`;
}
source += `export type P05SecretMainCommand = ${routes
  .filter((r) => r.surface === 'main' && r.owner === 'model' && r.auth === 'secret_input')
  .map((r) => JSON.stringify(r.method))
  .join(
    '|',
  )};\nexport type P05NonSecretMainCommand = Exclude<P05MainCommand,P05SecretMainCommand>;\n`;
for (const scope of ['main', 'core']) {
  const map = Object.fromEntries(
    routes.filter((r) => r.surface === scope).map((r) => [r.method, r]),
  );
  source += `export const ${scope.toUpperCase()}_ROUTES = ${JSON.stringify(map)} as const;\n`;
  source += `export function ${scope}Route(method:string) {return Object.hasOwn(${scope.toUpperCase()}_ROUTES,method)?${scope.toUpperCase()}_ROUTES[method as keyof typeof ${scope.toUpperCase()}_ROUTES]:undefined;}\n`;
}
source += `export const ROUTE_COUNTS = ${JSON.stringify({ main: routes.filter((r) => r.surface === 'main').length, core: routes.filter((r) => r.surface === 'core').length })} as const;\n`;
source = await format(source, {
  parser: 'typescript',
  singleQuote: true,
  printWidth: 100,
  endOfLine: 'lf',
});
await mkdir(dirname(output), { recursive: true });
await writeFile(output, source);
console.log(`generated ${output} (${routes.length} route bindings)`);

// Include only the Rust schema closure used by the readiness boundary.
const aggregate = JSON.parse(
  await readFile(resolve(dirname(input), '../schemas/aura-contracts.schema.json'), 'utf8'),
);
const idTypes = JSON.parse(
  await readFile(resolve(dirname(input), '../identity/id-prefix-registry.v1.json'), 'utf8'),
);
await generateAgentBoundary(aggregate, routes, dirname(output), idTypes);
await generateAgentBoundary(aggregate, routes, dirname(output), idTypes, 'git');
await generateAgentBoundary(aggregate, routes, dirname(output), idTypes, 'package');
await generateAgentBoundary(aggregate, routes, dirname(output), idTypes, 'memory');
await generateRecoveryBoundary(aggregate, routes, dirname(output));
await generateToolCredentialBoundary(aggregate, routes, dirname(output));
const definitions = {};
function visit(value) {
  if (!value || typeof value !== 'object') return;
  for (const [key, item] of Object.entries(value)) {
    if (key === '$ref' && item.startsWith('#/$defs/')) {
      const name = item.slice(8);
      if (!definitions[name]) {
        definitions[name] = aggregate.$defs[name];
        if (!definitions[name]) throw Error('missing schema ' + name);
        visit(definitions[name]);
      }
    } else visit(item);
  }
}
visit({ $ref: '#/$defs/RuntimeReadiness' });
const schema = { $schema: aggregate.$schema, $ref: '#/$defs/RuntimeReadiness', $defs: definitions };

const ajv = new Ajv2020({ strict: true, allErrors: true, code: { source: true, esm: true } });
const validate = ajv.compile(schema);
await writeFile(
  resolve(dirname(output), 'readiness-validator.ts'),
  await format(
    '// @ts-nocheck\n/* eslint-disable */\n/** Generated from Rust schema using AJV standalone. Do not edit. */\n' +
      standaloneCode(ajv, validate),
    { parser: 'typescript', singleQuote: true, printWidth: 100, endOfLine: 'lf' },
  ),
);
