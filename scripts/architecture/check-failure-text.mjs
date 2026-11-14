// Fails when a crate that answers the desktop gains a failure text that is not `code: 中文句`,
// or when a file holds fewer than its baseline still allows (see failure-text.mjs).
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { failureTextCounts, failureTextProblems } from './failure-text.mjs';

const root = resolve(import.meta.dirname, '../..');
const baseline = JSON.parse(
  readFileSync(resolve(root, 'scripts/architecture/failure-text-baseline.json'), 'utf8'),
);
const counts = failureTextCounts(root);
const problems = failureTextProblems(counts, baseline.files);
if (problems.length) throw new Error(`failure text guard failed:\n${problems.join('\n')}`);
const left = Object.values(counts).reduce((sum, count) => sum + count, 0);
console.log(`failure text guard passed (${left} older texts still to rewrite, none added)`);
