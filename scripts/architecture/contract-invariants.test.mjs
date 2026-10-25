// Security properties of the IPC routes and of the contracts behind them that hold for every
// version: who may call what, which actions stay with the user, and which inputs carry no path.
// contract-baseline.mjs pins the exact contracts; these tests say why some of them must not
// change.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

const ROOT = resolve(import.meta.dirname, '../..');
const read = (path) => JSON.parse(readFileSync(resolve(ROOT, path), 'utf8'));
const routes = () => read('resources/ipc/routes.v1.json');
const definitions = () => read('resources/schemas/aura-contracts.schema.json').$defs;
const release = () =>
  read('resources/tools/builtin-agent-release.v1.json').entries.map(
    (entry) => entry.definition.canonical_name,
  );
const surfaces = (method) =>
  routes()
    .filter((route) => route.method === method)
    .map((route) => route.surface)
    .sort();

const SECRET_INPUTS = [
  'agent.mcp.secret.set',
  'models.endpoint.configure_credential',
  'models.endpoint.create_with_credential',
  'models.endpoint.replace_credential',
  'terminal.user.sensitive.submit',
  'tools.credential.create',
  'tools.credential.rotate',
];

test('Main serves the renderer and Core serves only Main; no route is test-only', () => {
  for (const route of routes()) {
    const name = `${route.surface}:${route.method}`;
    assert.ok(['main', 'core'].includes(route.surface), name);
    assert.equal(route.test_only, false, name);
    if (route.auth === 'secret_input') continue;
    assert.equal(
      route.auth,
      route.surface === 'main' ? 'trusted_renderer' : 'authenticated_main',
      name,
    );
  }
});

test('secrets travel only on the secret input routes, on both surfaces', () => {
  const secret = routes().filter((route) => route.auth === 'secret_input');
  assert.deepEqual([...new Set(secret.map((route) => route.method))].sort(), SECRET_INPUTS);
  for (const method of SECRET_INPUTS) {
    const both = routes().filter((route) => route.method === method);
    assert.deepEqual(both.map((route) => route.surface).sort(), ['core', 'main'], method);
    for (const route of both) {
      assert.equal(route.auth, 'secret_input', `${route.surface}:${method}`);
      assert.equal(route.idempotency, 'command_invocation', `${route.surface}:${method}`);
    }
  }
});

test('the clipboard, outside files and links, dialogs and folders are Main only', () => {
  const mainOnly = [
    'agent.clipboard.write',
    'agent.files.open_outside',
    'agent.links.open',
    'memory.maintenance.open_log',
  ];
  const dialogs = routes()
    .map((route) => route.method)
    .filter((method) => /\.(select_directory|select_archive|open_directory)$/u.test(method));
  assert.ok(dialogs.length >= 8);
  for (const method of [...mainOnly, ...dialogs]) {
    // No Core route exists, so nothing the model calls can reach it.
    assert.deepEqual(surfaces(method), ['main'], method);
  }
  for (const method of mainOnly.filter((method) => method !== 'memory.maintenance.open_log')) {
    assert.equal(
      routes().find((route) => route.method === method).idempotency,
      'command_invocation',
      method,
    );
  }
});

test('host results, attachment lookups and the exit notice reach Core from Main only', () => {
  for (const method of [
    'agent.attachments.locate',
    'browser.host.result',
    'media.host.read',
    'media.host.result',
    'runtime.prepare_exit',
  ]) {
    assert.deepEqual(surfaces(method), ['core'], method);
  }
  const exit = routes().find((route) => route.method === 'runtime.prepare_exit');
  assert.equal(exit.idempotency, 'transient_input');
  assert.equal(exit.cancellation, 'none');
  assert.equal(exit.request_type, 'Record<string, never>');
  const media = routes().filter((route) => route.method.startsWith('media.'));
  assert.deepEqual(media.map((route) => route.method).sort(), [
    'media.host.read',
    'media.host.result',
  ]);
});

test('Hook, MCP, Skill and plugin settings: reads stay reads, changes are commands, no model tool', () => {
  const reads = {
    'agent.hooks.': ['list', 'preview', 'invocations', 'select_directory'],
    'agent.mcp.': ['list', 'preview', 'describe'],
    'agent.skills.': ['list', 'preview', 'select_directory', 'select_archive'],
    'agent.plugins.': ['list', 'preview', 'select_directory', 'select_archive'],
  };
  for (const [prefix, verbs] of Object.entries(reads)) {
    const owned = routes().filter((route) => route.method.startsWith(prefix));
    assert.ok(owned.length > 0, prefix);
    for (const route of owned) {
      if (route.auth === 'secret_input') continue;
      const verb = route.method.slice(prefix.length);
      assert.equal(
        route.idempotency,
        verbs.includes(verb) ? 'read_only' : 'command_invocation',
        `${route.surface}:${route.method}`,
      );
    }
  }
  // Trusting, enabling or importing these is never something the model can do.
  for (const name of release()) {
    assert.ok(!/(^|\.)(hooks?|mcp|plugins?)(\.|$)/u.test(name), name);
    assert.ok(!/^skills\.(import|trust|untrust|set_enabled|uninstall)/u.test(name), name);
  }
});

test('branching, archiving, deleting and restoring conversations are user commands', () => {
  for (const method of [
    'agent.session.branch',
    'session.archive',
    'session.delete',
    'session.restore',
  ]) {
    assert.deepEqual(surfaces(method), ['core', 'main'], method);
    for (const route of routes().filter((route) => route.method === method)) {
      assert.equal(route.idempotency, 'command_invocation', method);
    }
  }
  assert.deepEqual(surfaces('workspace.delete'), ['main']);
  for (const name of release()) {
    assert.ok(!/^(session|conversation)\.(branch|archive|delete|restore)/u.test(name), name);
  }
});

test('a question touch and a turn-activity read are never replayed or cancelled', () => {
  const touch = routes().filter((route) => route.method === 'agent.query.touch');
  const activity = routes().filter((route) => route.method === 'agent.sessions.activity');
  assert.equal(touch.length, 2);
  assert.equal(activity.length, 2);
  for (const route of touch) assert.equal(route.idempotency, 'transient_input');
  for (const route of activity) assert.equal(route.idempotency, 'read_only');
  for (const route of [...touch, ...activity]) assert.equal(route.cancellation, 'none');
});

test('memory exports are Core-only transport, never a renderer API', () => {
  const memory = routes().filter((route) => route.owner === 'memory');
  assert.ok(memory.some((route) => route.surface === 'main'));
  for (const route of memory.filter((route) => route.surface === 'main')) {
    assert.ok(!route.method.startsWith('memory.export.'), route.method);
  }
  assert.ok(memory.some((route) => route.method.startsWith('memory.export.')));
});

test('attachment and media requests name a conversation and carry no path', () => {
  const defs = definitions();
  for (const name of ['AttachmentTextRequest', 'AttachmentLocateRequest']) {
    assert.equal(defs[name].additionalProperties, false, name);
    assert.ok(!Object.hasOwn(defs[name].properties, 'path'), name);
    assert.ok(defs[name].required.includes('session_id'), name);
  }
  for (const name of ['MediaHostCommandV1', 'MediaHostReadRequestV1', 'MediaHostResultV1']) {
    assert.equal(defs[name].additionalProperties, false, name);
    assert.ok(!Object.hasOwn(defs[name].properties, 'path'), name);
    for (const field of ['command_id', 'runtime_instance_id', 'session_id']) {
      assert.ok(defs[name].required.includes(field), `${name}: ${field}`);
    }
  }
});

test('an outside file is attached only with its own grant; a workspace file carries none', () => {
  const variants = definitions().AgentMessageAttachment.oneOf;
  const external = variants.find((variant) => variant.properties.kind.const === 'external_file');
  assert.ok(external);
  assert.ok(external.required.includes('external_grant_id'));
  assert.ok(external.required.includes('path'));
  assert.equal(external.additionalProperties, false);
  const workspace = variants.find((variant) => variant.properties.kind.const === 'file');
  assert.ok(!Object.hasOwn(workspace.properties, 'external_grant_id'));
});

test('a deletion that skips the Recycle Bin is its own risk, and fs.delete takes only a path', () => {
  const defs = definitions();
  const risks = defs.OperationRiskFlag.enum ?? defs.OperationRiskFlag.oneOf.map((v) => v.const);
  assert.ok(risks.includes('permanent_file_deletion'));
  assert.equal(defs.AgentFileDeleteInput.additionalProperties, false);
  assert.deepEqual(defs.AgentFileDeleteInput.required, ['path']);
  const entry = read('resources/tools/builtin-agent-release.v1.json').entries.find(
    (e) => e.definition.canonical_name === 'fs.delete',
  );
  assert.equal(entry.definition.execution_semantics.may_create_persistent_effects, true);
});
