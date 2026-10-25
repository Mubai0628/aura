import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { expect, test } from 'vitest';
import { validateGitWriteContract } from './generated/git-validator.js';

const fixture = JSON.parse(
  readFileSync(
    resolve(import.meta.dirname, '../../../fixtures/contracts/git-writes.v1.json'),
    'utf8',
  ),
) as {
  commit_inputs: Record<string, unknown>[];
  branch_inputs: Record<string, unknown>[];
  commit_result: unknown;
  branch_result: unknown;
};
const valid = (command: string, value: unknown): boolean =>
  validateGitWriteContract({ command, value });

test('the generated validator accepts every Rust local-write fixture', () => {
  fixture.commit_inputs.forEach((value) => expect(valid('git.commit.input', value)).toBe(true));
  fixture.branch_inputs.forEach((value) => expect(valid('git.branch.input', value)).toBe(true));
  expect(valid('git.commit.result', fixture.commit_result)).toBe(true);
  expect(valid('git.branch.result', fixture.branch_result)).toBe(true);
});

test('write inputs cannot add force, hook bypass, a program or an environment override', () => {
  for (const key of ['amend', 'no_verify', 'program', 'environment']) {
    expect(valid('git.commit.input', { ...fixture.commit_inputs[0], [key]: true })).toBe(false);
  }
  for (const value of [
    { kind: 'switch', name: 'main', force: true },
    { kind: 'create', name: 'new', no_verify: true },
    { kind: 'create', name: '' },
    { ...fixture.commit_inputs[0], message: '' },
  ])
    expect(valid('kind' in value ? 'git.branch.input' : 'git.commit.input', value)).toBe(false);
});

test('successful mutation receipts require the bounded post-operation status', () => {
  for (const [command, source] of [
    ['git.commit.result', fixture.commit_result],
    ['git.branch.result', fixture.branch_result],
  ] as const) {
    const incomplete = { ...(source as Record<string, unknown>) };
    delete incomplete.status;
    expect(valid(command, incomplete)).toBe(false);
  }
});
