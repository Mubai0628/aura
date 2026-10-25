// The contracts Aura's processes and tools exchange, as one inventory with a hash per entry: every
// schema definition, IPC route, built-in agent tool, built-in tool definition and registered ID
// prefix. The committed inventory (contract-baseline.json) is what the tree must match, so a
// contract change updates it in the same commit and shows up as a reviewed diff:
//
//   node scripts/architecture/contract-baseline.mjs                    check
//   node scripts/architecture/contract-baseline.mjs --write            accept additions and changes
//   node scripts/architecture/contract-baseline.mjs --write --allow-removal
//
// Removing an entry breaks readers that still use it, so accepting a removal is a separate flag.
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const ROOT = resolve(import.meta.dirname, '../..');
export const BASELINE = 'scripts/architecture/contract-baseline.json';

const sorted = (value) =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? Object.fromEntries(
        Object.keys(value)
          .sort()
          .map((key) => [key, sorted(value[key])]),
      )
    : Array.isArray(value)
      ? value.map(sorted)
      : value;
export const digest = (value) =>
  createHash('sha256')
    .update(JSON.stringify(sorted(value)))
    .digest('hex');

const keyed = (entries, key, label) => {
  const out = {};
  for (const entry of entries) {
    const name = key(entry);
    if (typeof name !== 'string' || !name) throw new Error(`${label}: an entry has no key`);
    if (Object.hasOwn(out, name)) throw new Error(`${label}: duplicate entry ${name}`);
    out[name] = digest(entry);
  }
  return out;
};

/** Where each section comes from and how its entries are named. */
export const SECTIONS = {
  schema: {
    path: 'resources/schemas/aura-contracts.schema.json',
    entries: (value) => {
      const { $defs: definitions, ...catalog } = value;
      return {
        '(catalog)': digest(catalog),
        ...Object.fromEntries(
          Object.entries(definitions).map(([name, definition]) => [name, digest(definition)]),
        ),
      };
    },
  },
  routes: {
    path: 'resources/ipc/routes.v1.json',
    entries: (value) => keyed(value, (route) => `${route.surface}:${route.method}`, 'routes'),
  },
  agent_tools: {
    path: 'resources/tools/builtin-agent-release.v1.json',
    entries: ({ entries, ...release }) => ({
      '(release)': digest(release),
      ...keyed(entries, (entry) => entry.definition?.canonical_name, 'agent_tools'),
    }),
  },
  tool_definitions: {
    path: 'resources/tools/builtin-tool-definition-set.v1.json',
    entries: ({ entries, ...set }) => ({
      '(set)': digest(set),
      ...keyed(entries, (entry) => entry.definition?.builtin_key, 'tool_definitions'),
    }),
  },
  tool_ids: {
    path: 'resources/tools/builtin-tool-id-registry.v1.json',
    entries: ({ entries, ...registry }) => ({
      '(registry)': digest(registry),
      ...keyed(entries, (entry) => entry.builtin_key, 'tool_ids'),
    }),
  },
  id_prefixes: {
    path: 'resources/identity/id-prefix-registry.v1.json',
    entries: ({ entries, ...registry }) => ({
      '(registry)': digest(registry),
      ...keyed(entries, (entry) => entry.prefix, 'id_prefixes'),
    }),
  },
};

/** The inventory of the given documents, by section; `read` returns a parsed document by path. */
export function inventory(read) {
  return Object.fromEntries(
    Object.entries(SECTIONS).map(([name, section]) => [
      name,
      sorted(section.entries(read(section.path))),
    ]),
  );
}

/** What differs from `baseline`: added, removed and changed entries, by section. */
export function compare(baseline, current) {
  const changes = { added: [], removed: [], changed: [] };
  for (const name of Object.keys(SECTIONS)) {
    const before = baseline[name] ?? {};
    const after = current[name] ?? {};
    for (const entry of Object.keys(after)) {
      if (!Object.hasOwn(before, entry)) changes.added.push(`${name} ${entry}`);
      else if (before[entry] !== after[entry]) changes.changed.push(`${name} ${entry}`);
    }
    for (const entry of Object.keys(before)) {
      if (!Object.hasOwn(after, entry)) changes.removed.push(`${name} ${entry}`);
    }
  }
  return changes;
}

export const unchanged = (changes) =>
  !changes.added.length && !changes.removed.length && !changes.changed.length;

/** The baseline to write for `current`; a removal needs `allowRemoval`. */
export function accept(baseline, current, { allowRemoval = false } = {}) {
  const changes = compare(baseline, current);
  if (changes.removed.length && !allowRemoval) {
    throw new Error(`removed contracts need --allow-removal:\n  ${changes.removed.join('\n  ')}`);
  }
  return { format: 1, sections: current };
}

const readFromTree = (path) => JSON.parse(readFileSync(resolve(ROOT, path), 'utf8'));
export const readBaseline = () => readFromTree(BASELINE);

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const current = inventory(readFromTree);
  const first = process.argv.includes('--write') && !existsSync(resolve(ROOT, BASELINE));
  const baseline = first ? { sections: {} } : readBaseline();
  const changes = compare(baseline.sections, current);
  if (process.argv.includes('--write')) {
    const next = accept(baseline.sections, current, {
      allowRemoval: process.argv.includes('--allow-removal'),
    });
    writeFileSync(resolve(ROOT, BASELINE), `${JSON.stringify(next, null, 2)}\n`);
    console.log(
      `contract baseline written: ${changes.added.length} added, ${changes.changed.length} changed, ${changes.removed.length} removed`,
    );
  } else if (!unchanged(changes)) {
    const lines = [];
    for (const kind of ['added', 'changed', 'removed']) {
      for (const entry of changes[kind]) lines.push(`  ${kind}: ${entry}`);
    }
    console.error(
      `contracts differ from ${BASELINE}:\n${lines.join('\n')}\n` +
        'If the change is intended, run node scripts/architecture/contract-baseline.mjs --write ' +
        'and commit the baseline with it.',
    );
    process.exit(1);
  } else {
    const counts = Object.entries(current).map(
      ([name, entries]) => `${name} ${Object.keys(entries).length}`,
    );
    console.log(`contracts match the baseline (${counts.join(', ')})`);
  }
}
