import { access, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const currentContractVersion = { major: 1, minor: 7 };
const readJson = async (relativePath) =>
  JSON.parse(await readFile(resolve(root, relativePath), 'utf8'));

const assertVersion = (value, label) => {
  if (
    value?.major !== currentContractVersion.major ||
    !Number.isInteger(value?.minor) ||
    value.minor < 0 ||
    value.minor > currentContractVersion.minor
  ) {
    throw new Error(
      `${label} must use a supported contract version with major ${currentContractVersion.major} and minor at most ${currentContractVersion.minor}.`,
    );
  }
};

const assertSafePath = (value, label) => {
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    value.includes('..') ||
    value.startsWith('/') ||
    /^[A-Za-z]:/.test(value)
  ) {
    throw new Error(`${label} must be a safe relative path.`);
  }
};

const promptManifest = await readJson('resources/prompts/manifest.json');
assertVersion(promptManifest.manifest_version, 'Prompt manifest');
if (!Array.isArray(promptManifest.modules)) {
  throw new Error('Prompt manifest modules must be an array.');
}

const promptIds = new Set();
for (const module of promptManifest.modules) {
  if (typeof module.id !== 'string' || module.id.length === 0 || promptIds.has(module.id)) {
    throw new Error(`Prompt module ID is missing or duplicated: ${String(module.id)}`);
  }
  promptIds.add(module.id);
  assertSafePath(module.path, `Prompt module ${module.id}`);
  await access(resolve(root, 'resources/prompts', module.path));
}

const schemaManifest = await readJson('resources/schemas/manifest.json');
assertVersion(schemaManifest.manifest_version, 'Schema manifest');
if (!Array.isArray(schemaManifest.schemas) || schemaManifest.schemas.length === 0) {
  throw new Error('Schema manifest must contain at least one schema.');
}

const schemaIds = new Set();
for (const schema of schemaManifest.schemas) {
  if (typeof schema.id !== 'string' || schema.id.length === 0 || schemaIds.has(schema.id)) {
    throw new Error(`Schema ID is missing or duplicated: ${String(schema.id)}`);
  }
  schemaIds.add(schema.id);
  assertSafePath(schema.path, `Schema ${schema.id}`);
  await access(resolve(root, 'resources/schemas', schema.path));
  if (
    schema.content_hash?.algorithm !== 'blake3' ||
    !/^[0-9a-f]{64}$/.test(schema.content_hash.hex)
  ) {
    throw new Error(`Schema ${schema.id} must have a canonical BLAKE3 content hash.`);
  }
}

const componentManifest = await readJson('resources/component-manifest.example.json');
assertVersion(componentManifest.manifest_version, 'Component manifest');
if (!Array.isArray(componentManifest.components) || componentManifest.components.length === 0) {
  throw new Error('Component manifest must contain at least one component.');
}

const requiredComponentKinds = new Set([
  'desktop',
  'main_pipe_bridge',
  'core',
  'sandbox_host',
  'egress_proxy',
  'elevated_helper',
]);
const componentIds = new Set();
const componentKinds = new Set();
for (const component of componentManifest.components) {
  if (
    typeof component.component_id !== 'string' ||
    component.component_id.length === 0 ||
    componentIds.has(component.component_id)
  ) {
    throw new Error(`Component ID is missing or duplicated: ${String(component.component_id)}`);
  }
  componentIds.add(component.component_id);
  componentKinds.add(component.kind);
  assertSafePath(component.relative_path, `Component ${component.component_id}`);
  assertVersion(component.protocol_version, `Component ${component.component_id}`);
  if (
    component.content_hash?.algorithm !== 'sha256' ||
    !/^[0-9a-f]{64}$/u.test(component.content_hash.hex)
  ) {
    throw new Error(`Component ${component.component_id} must have a canonical SHA-256 hash.`);
  }
}
for (const kind of requiredComponentKinds) {
  if (!componentKinds.has(kind)) throw new Error(`Component manifest is missing ${kind}.`);
}

console.log('Prompt, schema, and component manifest placeholders are valid.');
