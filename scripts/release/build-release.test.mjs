import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { requireProductionTrust } from './build-release.mjs';

test('a production release needs the root in every program that checks its signature', () => {
  const folder = mkdtempSync(join(tmpdir(), 'aura-release-trust-'));
  try {
    const root = Buffer.from('the production root certificate');
    const program = Buffer.concat([Buffer.from('MZ program text '), root, Buffer.from(' more')]);
    for (const name of ['aura-setup.exe', 'aura-core.exe', 'aura-elevated-helper.exe']) {
      writeFileSync(join(folder, name), program);
    }
    requireProductionTrust(folder, root);
    writeFileSync(join(folder, 'aura-core.exe'), Buffer.from('MZ built without the switch'));
    assert.throws(
      () => requireProductionTrust(folder, root),
      /aura-core\.exe carries no production release root/u,
    );
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
});
