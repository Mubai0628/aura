import assert from 'node:assert/strict';
import test from 'node:test';
import { compareBaseline, dependencyGraph, violations } from './graph.mjs';

test('the R06 spike has no SQL path and cannot enter production Core', () => {
  const found = violations({
    'aura-core': ['aura-restricted-user-spike'],
    'aura-restricted-user-spike': ['adapter'],
    adapter: ['rusqlite'],
  });
  assert.ok(found.includes('spike:aura-core->aura-restricted-user-spike'));
  assert.ok(found.includes('runner:aura-restricted-user-spike->adapter->rusqlite'));
});

test('retired libgit2 cannot return through its former owner or the native adapter', () => {
  assert.deepEqual(
    violations({
      'aura-core': ['git2'],
      'aura-git-service': ['git2'],
      'aura-backend-windows-user': ['libgit2-sys'],
    }),
    [
      'git-native-owner:aura-backend-windows-user->libgit2-sys',
      'git-native-owner:aura-core->git2',
      'git-native-owner:aura-git-service->git2',
    ],
  );
});

test('Core Broker cannot acquire domain, SQL or process implementation dependencies', () => {
  const found = violations({
    'aura-backend-core': ['aura-tool-contract', 'aura-sandbox-host', 'adapter'],
    adapter: ['rusqlite'],
  });
  assert.ok(found.includes('kernel:aura-backend-core->aura-tool-contract'));
  assert.ok(found.includes('kernel:aura-backend-core->aura-sandbox-host'));
  assert.ok(found.includes('broker:aura-backend-core->adapter->rusqlite'));
});

test('V2 stores reject legacy/importer paths and runners cannot open the V2 database', () => {
  const result = violations({
    'aura-persistence-v2': ['adapter'],
    adapter: ['aura-persistence'],
    'aura-model-store': ['aura-v2-importer'],
    'aura-command-runner': ['aura-persistence-v2'],
  });
  assert.ok(result.includes('v2-store:aura-persistence-v2->adapter->aura-persistence'));
  assert.ok(result.includes('v2-store:aura-model-store->aura-v2-importer'));
  assert.ok(result.includes('runner:aura-command-runner->aura-persistence-v2'));
});

test('Cargo aliases, optional and target dependencies use the real package name; dev edges are excluded', () => {
  const graph = dependencyGraph({
    workspace_members: ['runner'],
    packages: [
      {
        id: 'runner',
        name: 'aura-command-runner',
        dependencies: [
          {
            name: 'rusqlite',
            rename: 'database',
            kind: null,
            optional: true,
            target: 'cfg(windows)',
          },
          { name: 'aura-persistence', kind: 'dev' },
        ],
      },
    ],
  });
  assert.deepEqual(graph['aura-command-runner'], ['rusqlite']);
  assert.deepEqual(violations(graph), ['runner:aura-command-runner->rusqlite']);
});

test('a new indirect SQL route cannot reuse an existing legacy exception', () => {
  const original = { 'aura-sandbox-host': ['aura-persistence'], 'aura-persistence': ['rusqlite'] };
  const exceptions = violations(original).map((violation) => ({ violation, remove_by: 'R14' }));
  const changed = {
    ...original,
    'aura-sandbox-host': ['aura-persistence', 'adapter'],
    adapter: ['rusqlite'],
  };
  assert.deepEqual(compareBaseline(changed, exceptions).added, [
    'runner:aura-sandbox-host->adapter->rusqlite',
  ]);
});

test('kernel reverse dependencies and cycles fail independently of runner exceptions', () => {
  const graph = {
    'aura-security-contract': ['aura-tool-contract'],
    'aura-tool-contract': ['aura-security-contract'],
  };
  const result = violations(graph);
  assert.ok(result.includes('kernel:aura-security-contract->aura-tool-contract'));
  assert.ok(result.some((item) => item.startsWith('cycle:')));
});

test('shared primitives and extracted domain contracts cannot hide a reverse domain edge', () => {
  assert.deepEqual(
    violations({
      'aura-artifact-contract': ['aura-domain-common', 'aura-tool-contract'],
      'aura-domain-common': ['aura-workspace-domain'],
      'aura-runtime-contract': ['aura-domain-common', 'aura-ipc'],
      'aura-workspace-domain': ['aura-domain-common', 'aura-persistence'],
    }).filter((entry) => entry.startsWith('kernel:')),
    [
      'kernel:aura-artifact-contract->aura-tool-contract',
      'kernel:aura-domain-common->aura-workspace-domain',
      'kernel:aura-runtime-contract->aura-ipc',
      'kernel:aura-workspace-domain->aura-persistence',
    ],
  );
});

test('removing old violations is reported without calling absent V2 kernels implemented', () => {
  const result = compareBaseline({ 'aura-command-runner': ['aura-execution-contract'] }, [
    { violation: 'runner:aura-command-runner->rusqlite', remove_by: 'R06' },
  ]);
  assert.deepEqual(result.added, []);
  assert.equal(result.resolved.length, 1);
  assert.ok(result.pendingKernels.includes('aura-security-contract'));
});
test('product crates cannot depend on the evaluation harness, including transitively', () => {
  assert.deepEqual(violations({ 'aura-core': ['aura-tool-eval'], 'aura-tool-eval': [] }), [
    'evaluation:aura-core->aura-tool-eval',
  ]);
  assert.deepEqual(
    violations({
      'aura-core': ['aura-model-domain'],
      'aura-model-domain': ['aura-tool-eval'],
      'aura-tool-eval': [],
    }),
    [
      'evaluation:aura-core->aura-model-domain->aura-tool-eval',
      'evaluation:aura-model-domain->aura-tool-eval',
    ],
  );
});

test('the evaluation dependency prohibition also applies to product test dependencies', () => {
  const graph = dependencyGraph({
    workspace_members: ['core'],
    packages: [
      {
        id: 'core',
        name: 'aura-core',
        dependencies: [{ name: 'aura-tool-eval', kind: 'dev' }],
      },
    ],
  });
  assert.ok(violations(graph).includes('evaluation:aura-core->aura-tool-eval'));
});
