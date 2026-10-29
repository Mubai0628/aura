import assert from 'node:assert/strict';
import test from 'node:test';
import {
  crateViolations,
  handwrittenSource,
  lineCount,
  productionSource,
  sizeViolations,
} from './file-size.mjs';

test('new production and test files cannot exceed 1000 physical lines', () => {
  const files = ['crates/tool/src/lib.rs', 'crates/tool/tests/behavior.rs'];
  assert.equal(sizeViolations(files, {}, () => 'code\n'.repeat(1000)).length, 0);
  assert.equal(sizeViolations(files, {}, () => 'code\n'.repeat(1001)).length, 2);
});

test('existing oversized files may shrink but cannot grow beyond their adopted ceiling', () => {
  const file = 'crates/tool/src/legacy.rs';
  const limits = { [file]: 1234 };
  for (const lines of [500, 1000, 1234]) {
    assert.deepEqual(
      sizeViolations([file], limits, () => '\n'.repeat(lines)),
      [],
    );
  }
  assert.deepEqual(
    sizeViolations([file], limits, () => '\n'.repeat(1235)),
    [{ path: file, lines: 1235, ceiling: 1234 }],
  );
});

test('only generated directories are exempt and test sources are included', () => {
  assert.equal(handwrittenSource('packages/contracts/src/generated/contracts.ts'), false);
  assert.equal(handwrittenSource('crates/tool/tests/generated_name.rs'), true);
  assert.equal(handwrittenSource('apps/desktop/tests/behavior.spec.ts'), true);
  assert.equal(handwrittenSource('docs/guide.md'), false);
});

test('physical line counting agrees across line endings and final newline', () => {
  assert.equal(lineCount(''), 0);
  for (const content of ['one\ntwo\n', 'one\ntwo', 'one\r\ntwo\r\n']) {
    assert.equal(lineCount(content), 2);
  }
});

test('module variants, native sources and styles share the handwritten guard', () => {
  for (const path of [
    'preload/index.cts',
    'tests/provider.d.mts',
    'renderer/app.css',
    'native/bridge.cpp',
    'native/bridge.h',
    'renderer/index.html',
  ]) {
    assert.equal(handwrittenSource(path), true, path);
  }
});

test('the production code of a capped crate may shrink but cannot grow beyond its ceiling', () => {
  const files = [
    'crates/aura-core/src/lib.rs',
    'crates/aura-core/src/agent/mod.rs',
    'crates/aura-core/src/agent/tests/behavior.rs',
    'crates/aura-core/tests/integration.rs',
    'crates/aura-store/src/lib.rs',
  ];
  const read = () => 'code\n'.repeat(10);
  const ceilings = (lines) => ({ 'crates/aura-core': lines });
  assert.deepEqual(crateViolations(files, ceilings(20), read), []);
  assert.deepEqual(crateViolations(files, ceilings(25), read), []);
  assert.deepEqual(crateViolations(files, ceilings(19), read), [
    { crate: 'crates/aura-core', lines: 20, ceiling: 19 },
  ]);
});

test('test directories and test files do not count as production code', () => {
  for (const path of [
    'crates/aura-core/src/lib.rs',
    'crates/aura-core/src/agent_application/history_memo.rs',
    'crates/aura-core/src/test_mode.rs',
  ]) {
    assert.equal(productionSource(path), true, path);
  }
  for (const path of [
    'crates/aura-core/src/agent_application/tests/fixture.rs',
    'crates/aura-core/src/agent_application/tests.rs',
    'crates/aura-core/src/ipc_lifecycle_tests.rs',
    'crates/aura-core/src/r03_tests/fixture.rs',
    'crates/aura-core/src/package_services/test_support.rs',
    'crates/aura-core/src/p07_test_support.rs',
    'crates/aura-core/tests/store_migration_check.rs',
    'crates/aura-core/src/generated/routes.rs',
  ]) {
    assert.equal(productionSource(path), false, path);
  }
});
