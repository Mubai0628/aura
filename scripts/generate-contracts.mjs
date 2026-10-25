import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { compile } from 'json-schema-to-typescript';
import { format } from 'prettier';

const argumentsByName = new Map();
for (let index = 2; index < process.argv.length; index += 2) {
  const name = process.argv[index];
  const value = process.argv[index + 1];
  if (name === undefined || value === undefined || !name.startsWith('--')) {
    throw new Error('Usage: node scripts/generate-contracts.mjs --schema <path> --output <path>');
  }
  argumentsByName.set(name, value);
}

const schemaArgument = argumentsByName.get('--schema');
const outputArgument = argumentsByName.get('--output');
if (schemaArgument === undefined || outputArgument === undefined) {
  throw new Error('Both --schema and --output are required.');
}

const schemaPath = resolve(schemaArgument);
const outputPath = resolve(outputArgument);
const schema = JSON.parse(await readFile(schemaPath, 'utf8'));
const generated = await compile(schema, 'AuraContractCatalog', {
  additionalProperties: false,
  bannerComment:
    '/* eslint-disable */\n' +
    '/** Generated from resources/schemas/aura-contracts.schema.json. Do not edit. */',
  declareExternallyReferenced: true,
  enableConstEnums: false,
  format: true,
  ignoreMinAndMaxItems: false,
  strictIndexSignatures: true,
  style: {
    bracketSpacing: true,
    endOfLine: 'lf',
    printWidth: 100,
    semi: true,
    singleQuote: true,
    tabWidth: 2,
    trailingComma: 'all',
    useTabs: false,
  },
  unreachableDefinitions: true,
  unknownAny: false,
});

const normalized = generated.replaceAll('\r\n', '\n').replace(/\s*$/, '\n');
await mkdir(dirname(outputPath), { recursive: true });
let existing;
try {
  existing = await readFile(outputPath, 'utf8');
} catch (error) {
  if (error?.code !== 'ENOENT') {
    throw error;
  }
}

if (existing !== normalized) {
  await writeFile(outputPath, normalized, 'utf8');
}

// The embedded catalog is pinned by its exact source bytes. Keep its historical
// formatting in the generation pipeline as well as its typed Rust contents.
const builtinArgument = argumentsByName.get('--builtin-definitions');
if (builtinArgument !== undefined) {
  const path = resolve(builtinArgument);
  const source = await readFile(path, 'utf8');
  const formatted = await format(source, {
    parser: 'json',
    endOfLine: 'lf',
    printWidth: 100,
    tabWidth: 2,
    useTabs: false,
  });
  if (formatted !== source) await writeFile(path, formatted, 'utf8');
}
