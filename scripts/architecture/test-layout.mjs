// One integration test program per crate. Cargo links every file directly under `tests/`, and
// every `tests/<name>/main.rs`, into a program of its own, with the crate and all its dependencies
// and a debug file each; 446 such programs made up most of a test build. A crate keeps its tests in
// one program, `tests/<name>/main.rs`, which declares one module per behaviour.
import { existsSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

/**
 * @param {string} root repository root
 * @param {string[]} crateFolders folders holding a Cargo.toml, relative to the root, with `/`
 * @returns {string[]} problems
 */
export function testLayoutProblems(root, crateFolders) {
  const problems = [];
  for (const folder of crateFolders) {
    const tests = join(root, folder, 'tests');
    if (!existsSync(tests)) continue;
    const programs = [];
    for (const name of readdirSync(tests)) {
      const path = join(tests, name);
      if (name.endsWith('.rs') && statSync(path).isFile()) programs.push(name);
      else if (statSync(path).isDirectory() && existsSync(join(path, 'main.rs')))
        programs.push(`${name}/main.rs`);
    }
    if (programs.length > 1) {
      problems.push(
        `${folder}/tests builds ${programs.length} test programs (${programs.sort().join(', ')}); ` +
          'keep one, tests/<name>/main.rs declaring one module per behaviour',
      );
    }
  }
  return problems;
}
