import assert from 'node:assert/strict';
import test from 'node:test';
import { coveredSource, locationViolations, stripComments } from './data-locations.mjs';

const kinds = (path, source) => locationViolations(path, source).map(({ kind }) => kind);

test('Rust that reads the profile folders or puts the data paths together is found', () => {
  const source = [
    'let local = env::var_os("LOCALAPPDATA")?;',
    'let roaming = std::env::var("APPDATA");',
    'let data = Path::new(&local).join("Aura").join("namespaces");',
    'let log = local.join("Aura")',
    '    .join("logs");',
    'let memory = profile.join(".aura/memories");',
    'let ui = roaming.join("@aura\\\\desktop");',
  ].join('\n');
  assert.deepEqual(kinds('crates/aura-x/src/lib.rs', source), [
    'environment',
    'environment',
    'data folder',
    'data folder',
    'global memory',
    'window profile',
  ]);
  assert.deepEqual(
    locationViolations('crates/aura-x/src/lib.rs', source).map(({ line }) => line),
    [1, 2, 3, 4, 6, 7],
  );
});

test('scripts that read the profile folders or put the data paths together are found', () => {
  const source = [
    'const local = process.env.LOCALAPPDATA;',
    "const roaming = process.env['APPDATA'];",
    "const data = resolve(local, 'Aura/namespaces', hash);",
    "const logs = join(local, 'Aura', 'logs');",
    "const memory = join(home, '.aura', 'memories');",
    "const ui = join(app.getPath('appData'), '@aura', 'desktop');",
  ].join('\n');
  assert.deepEqual(kinds('apps/desktop/electron-main/main.ts', source), [
    'environment',
    'environment',
    'environment',
    'data folder',
    'data folder',
    'global memory',
    'window profile',
  ]);
});

test('comments may name the places, and strings that only look like comments stay code', () => {
  const rust = [
    '//! Core writes `%LOCALAPPDATA%\\Aura\\logs\\core.jsonl` no longer.',
    '/* env::var_os("LOCALAPPDATA") */',
    'let pipe = r"\\\\.\\pipe\\Aura\\";',
    'let url = "https://example.test"; // "Aura/namespaces"',
  ].join('\n');
  assert.deepEqual(kinds('crates/aura-x/src/lib.rs', rust), []);
  assert.equal(stripComments(rust, 'rust').split('\n').length, 4);
  const script = "const url = 'https://example.test/Aura'; // process.env.APPDATA\n";
  assert.deepEqual(kinds('apps/desktop/electron-main/a.ts', script), []);
  const hidden = 'let s = "//"; let local = env::var_os("APPDATA");';
  assert.deepEqual(kinds('crates/aura-x/src/lib.rs', hidden), ['environment']);
});

test("a Rust file's inline test module may build the paths", () => {
  const source = [
    'pub fn product() {}',
    '#[cfg(test)]',
    'mod tests {',
    "    const BRACE: char = '}';",
    '    fn deep() { let s = "}"; let p = root.join("Aura").join("namespaces"); }',
    '}',
    'fn after() { let local = std::env::var_os("LOCALAPPDATA"); }',
  ].join('\n');
  const found = locationViolations('crates/aura-x/src/path.rs', source);
  assert.deepEqual(
    found.map(({ kind, line }) => [kind, line]),
    [['environment', 7]],
  );
});

test('the rule covers product code, not the crate that owns the locations or any test', () => {
  assert.equal(coveredSource('crates/aura-core/src/lib.rs'), true);
  assert.equal(coveredSource('apps/desktop/electron-main/main.ts'), true);
  assert.equal(coveredSource('crates/aura-data-location/src/resolve.rs'), false);
  assert.equal(coveredSource('crates/aura-tool-eval/src/lib.rs'), false);
  assert.equal(coveredSource('crates/aura-core/src/r03_tests.rs'), false);
  assert.equal(coveredSource('crates/aura-core/src/tests/options.rs'), false);
  assert.equal(coveredSource('crates/aura-launcher/tests/trial_launch.rs'), false);
  assert.equal(coveredSource('crates/aura-launcher/examples/fake_desktop.rs'), false);
  assert.equal(coveredSource('apps/desktop/electron-main/main-log.test.ts'), false);
  assert.equal(coveredSource('apps/desktop/electron-main/core-supervisor-tests/fixture.ts'), false);
  assert.equal(coveredSource('apps/desktop/renderer/src/app.ts'), false);
});
