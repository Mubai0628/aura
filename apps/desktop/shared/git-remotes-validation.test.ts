import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { expect, test } from 'vitest';
import { validateGitWriteContract } from './generated/git-validator.js';
const fixture = JSON.parse(
  readFileSync(
    resolve(import.meta.dirname, '../../../fixtures/contracts/git-remotes.v1.json'),
    'utf8',
  ),
) as {
  sync_inputs: unknown[];
  push_inputs: Record<string, unknown>[];
  result: Record<string, unknown>;
};
test('all generated Git remote fixtures validate without accepting arbitrary process controls', () => {
  fixture.sync_inputs.forEach((value) =>
    expect(validateGitWriteContract({ command: 'git.sync.input', value })).toBe(true),
  );
  fixture.push_inputs.forEach((value) =>
    expect(validateGitWriteContract({ command: 'git.push.input', value })).toBe(true),
  );
  for (const command of ['git.push.result', 'git.sync.result'])
    expect(validateGitWriteContract({ command, value: fixture.result })).toBe(true);
  for (const key of ['force', 'mirror', 'delete', 'no_verify', 'program', 'environment'])
    expect(
      validateGitWriteContract({
        command: 'git.push.input',
        value: { ...fixture.push_inputs[0], [key]: true },
      }),
    ).toBe(false);
  expect(
    validateGitWriteContract({ command: 'git.sync.input', value: { kind: 'pull', rebase: true } }),
  ).toBe(false);
  const missing = { ...fixture.result };
  delete missing.status;
  expect(validateGitWriteContract({ command: 'git.push.result', value: missing })).toBe(false);
});
