import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';
import {
  SECTIONS,
  accept,
  compare,
  digest,
  inventory,
  readBaseline,
  unchanged,
} from './contract-baseline.mjs';

const ROOT = resolve(import.meta.dirname, '../..');
const read = (path) => JSON.parse(readFileSync(resolve(ROOT, path), 'utf8'));
/** The tree's documents, with `edit` applied to fresh copies. */
const tree = (edit = () => {}) => {
  const documents = Object.fromEntries(
    Object.values(SECTIONS).map(({ path }) => [path, read(path)]),
  );
  edit(documents);
  return (path) => documents[path];
};

test('the tree matches the committed contract baseline', () => {
  const changes = compare(readBaseline().sections, inventory(tree()));
  assert.ok(unchanged(changes), JSON.stringify(changes, null, 2));
});

test('a new field and a new definition show as a changed and an added contract', () => {
  const current = inventory(
    tree((documents) => {
      const schema = documents[SECTIONS.schema.path];
      schema.$defs.AttachmentTextRequest.properties.unapproved = { type: 'string' };
      schema.$defs.UnapprovedDefinition = { type: 'string' };
    }),
  );
  const changes = compare(readBaseline().sections, current);
  assert.deepEqual(changes.changed, ['schema AttachmentTextRequest']);
  assert.deepEqual(changes.added, ['schema UnapprovedDefinition']);
  assert.deepEqual(changes.removed, []);
});

test("a route's wider authority is a change, and a removed route needs --allow-removal", () => {
  const current = inventory(
    tree((documents) => {
      const routes = documents[SECTIONS.routes.path];
      const locate = routes.find((route) => route.method === 'agent.attachments.locate');
      locate.auth = 'renderer';
      routes.splice(
        routes.findIndex((route) => route.method === 'agent.attachments.text'),
        1,
      );
    }),
  );
  const baseline = readBaseline().sections;
  const changes = compare(baseline, current);
  assert.deepEqual(changes.changed, ['routes core:agent.attachments.locate']);
  assert.equal(changes.removed.length, 1);
  assert.match(changes.removed[0], /^routes [a-z_]+:agent\.attachments\.text$/u);
  assert.throws(() => accept(baseline, current), /--allow-removal/u);
  assert.deepEqual(accept(baseline, current, { allowRemoval: true }).sections, current);
});

test('key order does not change a digest, and duplicate entries are refused', () => {
  assert.equal(digest({ a: 1, b: { c: 2, d: 3 } }), digest({ b: { d: 3, c: 2 }, a: 1 }));
  assert.notEqual(digest([1, 2]), digest([2, 1]));
  assert.throws(
    () =>
      inventory(
        tree((documents) => {
          const routes = documents[SECTIONS.routes.path];
          routes.push(structuredClone(routes[0]));
        }),
      ),
    /duplicate entry/u,
  );
});
