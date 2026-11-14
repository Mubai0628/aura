// Fails when a workspace crate builds more than one integration test program (see test-layout.mjs).
import { dirname, relative, resolve } from 'node:path';
import { capture } from './capture.mjs';
import { testLayoutProblems } from './test-layout.mjs';

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
const folders = metadata.packages.map((item) =>
  relative(root, dirname(item.manifest_path)).replaceAll('\\', '/'),
);
const problems = testLayoutProblems(root, folders);
if (problems.length)
  throw new Error(`more than one integration test program:\n${problems.join('\n')}`);
console.log(`test layout: ${folders.length} crates, at most one integration test program each`);
