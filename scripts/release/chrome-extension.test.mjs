import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { checkExtension, extensionTarget, nativeHostName } from './chrome-extension.mjs';

const TEST_NAMESPACE = 'aura.desktop.test.b6';

test('host names follow aura-chrome-domain: the released one, and one per test namespace', () => {
  assert.equal(nativeHostName('aura.desktop.production'), 'com.aura.chrome_control');
  // aura-chrome-domain's tests pin the same value for native_host_name.
  assert.equal(nativeHostName(TEST_NAMESPACE), 'com.aura.chrome_control.t2fbad0b3fbc4');
  assert.equal(extensionTarget('aura.desktop.production'), 'production');
  assert.equal(extensionTarget(TEST_NAMESPACE), 'test');
});

test('a release takes only the extension built for its namespace and version', async () => {
  const folder = mkdtempSync(join(tmpdir(), 'aura-release-extension-'));
  try {
    const keys = join(folder, 'keys');
    mkdirSync(keys);
    writeFileSync(join(keys, 'production.pub'), 'PRODUCTION-KEY\n');
    writeFileSync(join(keys, 'test.pub'), 'TEST-KEY\n');
    const build = (name, { key, version, host }) => {
      const at = join(folder, name);
      mkdirSync(join(at, 'chunks'), { recursive: true });
      writeFileSync(join(at, 'manifest.json'), JSON.stringify({ key, version }));
      writeFileSync(join(at, 'service-worker.js'), `const a={hostName:\`${host}\`};`);
      writeFileSync(join(at, 'chunks', 'shared.js'), 'export const b=1;');
      return at;
    };
    const testHost = nativeHostName(TEST_NAMESPACE);
    const good = build('good', { key: 'TEST-KEY', version: '0.1.4', host: testHost });
    const release = { namespace: TEST_NAMESPACE, version: '0.1.4', keys };
    assert.equal((await checkExtension(good, release)).version, '0.1.4');

    const production = build('production', {
      key: 'PRODUCTION-KEY',
      version: '0.1.4',
      host: 'com.aura.chrome_control',
    });
    await checkExtension(production, { ...release, namespace: 'aura.desktop.production' });
    await assert.rejects(checkExtension(production, release), /not the test build/u);

    const otherRun = build('other-run', {
      key: 'TEST-KEY',
      version: '0.1.4',
      host: nativeHostName('aura.desktop.test.other'),
    });
    await assert.rejects(
      checkExtension(otherRun, release),
      /reaches com\.aura\.chrome_control\.t/u,
    );
    const older = build('older', { key: 'TEST-KEY', version: '0.1.3', host: testHost });
    await assert.rejects(checkExtension(older, release), /version 0\.1\.3, not 0\.1\.4/u);
    await assert.rejects(
      checkExtension(join(folder, 'missing'), release),
      /no Chrome extension build/u,
    );
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
});
