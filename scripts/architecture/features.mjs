// Crates split out of aura-core open hooks for Core's scenario tests behind a `test-hooks`
// feature. Only aura-core's dev-dependencies may enable it, so no build of a shipped program
// carries one.
export const TEST_ONLY_FEATURES = ['test-hooks'];

/** The product builds of aura-core: default, the E2E driver and the acceptance barrier. */
export const PRODUCT_FEATURE_SETS = [[], ['p08-contract-driver'], ['acceptance-barrier']];

/** `cargo tree` arguments that print each package with its resolved features, dev edges off. */
export function treeArguments(features) {
  return [
    'tree',
    '-p',
    'aura-core',
    '-e',
    'no-dev',
    '--prefix',
    'none',
    '-f',
    '{p}|{f}',
    '--target',
    'x86_64-pc-windows-msvc',
    '--color',
    'never',
    // Not `--offline`: the listing reads every package of the graph, and a fresh checkout or CI
    // runner has only downloaded what it has built so far. With all locked packages cached,
    // Cargo makes no network request.
    '--locked',
    ...(features.length ? ['--features', features.join(',')] : []),
  ];
}

/** `<package>/<feature>` for every test-only feature in a `cargo tree -f "{p}|{f}"` listing. */
export function testOnlyFeatureViolations(tree) {
  const found = new Set();
  for (const line of tree.split(/\r?\n/u)) {
    // A repeated package ends in " (*)"; `treeArguments` turns colour off, so it is plain text.
    const plain = line.replace(/\s*\(\*\)\s*$/u, '');
    const [packageId, features = ''] = plain.split('|');
    const name = packageId.trim().split(' ')[0];
    if (!name) continue;
    for (const feature of features.split(',').map((item) => item.trim())) {
      if (TEST_ONLY_FEATURES.includes(feature)) found.add(`${name}/${feature}`);
    }
  }
  return [...found].sort();
}
