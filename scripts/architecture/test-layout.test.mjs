import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { testLayoutProblems } from './test-layout.mjs';

function crate(root, name, files) {
  for (const file of files) {
    const path = join(root, 'crates', name, file);
    mkdirSync(join(path, '..'), { recursive: true });
    writeFileSync(path, '');
  }
  return `crates/${name}`;
}

test('a crate with one test program, or none, passes', () => {
  const root = mkdtempSync(join(tmpdir(), 'aura-test-layout-'));
  try {
    const folders = [
      crate(root, 'one-file', ['Cargo.toml', 'tests/one_file.rs']),
      crate(root, 'one-folder', [
        'Cargo.toml',
        'tests/one/main.rs',
        'tests/one/reads.rs',
        'tests/support/mod.rs',
      ]),
      crate(root, 'none', ['Cargo.toml', 'src/lib.rs']),
    ];
    assert.deepEqual(testLayoutProblems(root, folders), []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('two programs in a crate are named, files and folders alike', () => {
  const root = mkdtempSync(join(tmpdir(), 'aura-test-layout-'));
  try {
    const folders = [
      crate(root, 'two', ['Cargo.toml', 'tests/b.rs', 'tests/a/main.rs', 'tests/a/x.rs']),
    ];
    const problems = testLayoutProblems(root, folders);
    assert.equal(problems.length, 1);
    assert.match(problems[0], /^crates\/two\/tests builds 2 test programs \(a\/main\.rs, b\.rs\)/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
