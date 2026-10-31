import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { compareBaseline, dependencyGraph } from './graph.mjs';
import { capture } from './capture.mjs';
import { PRODUCT_FEATURE_SETS, testOnlyFeatureViolations, treeArguments } from './features.mjs';

const root = resolve(import.meta.dirname, '../..');
const metadata = JSON.parse(
  capture(root, 'cargo', [
    'metadata',
    '--format-version',
    '1',
    '--no-deps',
    '--locked',
    '--offline',
  ]),
);
// The dependency paths that predate the current architecture. The list only shrinks: a path
// that no longer exists must be removed, and any other new path fails as a violation.
const exceptions = JSON.parse(
  await readFile(resolve(root, 'scripts/architecture/legacy-exceptions.json'), 'utf8'),
);
const result = compareBaseline(dependencyGraph(metadata), exceptions);
if (process.argv.includes('--require-kernels') && result.pendingKernels.length) {
  throw new Error(`every kernel crate is required: ${result.pendingKernels.join(', ')}`);
}
for (const item of exceptions) {
  if (
    !item.remove_by ||
    !item.reason ||
    !/^runner:aura-(sandbox-host|egress-proxy)->/.test(item.violation)
  ) {
    throw new Error(`invalid legacy exception: ${JSON.stringify(item)}`);
  }
}
if (result.resolved.length)
  throw new Error(`remove resolved legacy exceptions:\n${result.resolved.join('\n')}`);
if (result.added.length)
  throw new Error(`new architecture violations:\n${result.added.join('\n')}`);
for (const features of PRODUCT_FEATURE_SETS) {
  const found = testOnlyFeatureViolations(
    capture(root, 'cargo', treeArguments(features)).toString('utf8'),
  );
  if (found.length)
    throw new Error(
      `test-only features in the ${features.join(',') || 'default'} build of aura-core: ${found.join(', ')}`,
    );
}
console.log(
  JSON.stringify(
    {
      status: 'no_new_dependency_violations',
      remaining_legacy_violations: result.remaining,
      resolved_legacy_violations: result.resolved,
      not_yet_implemented: result.pendingKernels,
    },
    null,
    2,
  ),
);
