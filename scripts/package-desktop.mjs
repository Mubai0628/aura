import { spawnSync } from 'node:child_process';
import { cp, rm } from 'node:fs/promises';
import { relative, resolve, sep } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const desktopRoot = resolve(root, 'apps/desktop');
const electronBuilder = resolve(desktopRoot, 'node_modules/electron-builder/cli.js');
const electronSource = resolve(desktopRoot, 'node_modules/electron/dist');
const electronStaging = resolve(root, 'target/electron-dist-staging');
const environment = { ...process.env };

await rm(electronStaging, { force: true, recursive: true });
await cp(electronSource, electronStaging, {
  recursive: true,
  filter(source) {
    return relative(electronSource, source).split(sep).join('/') !== 'resources/default_app.asar';
  },
});

// electron-builder invokes pnpm again to collect production dependencies. When this
// script itself was reached through `corepack pnpm`, carrying COREPACK_ROOT into that
// nested invocation prevents pnpm's launcher from switching to the repository-pinned
// version. Remove only the ancestry marker; pnpm's strict version check stays enabled.
delete environment.COREPACK_ROOT;

const result = spawnSync(
  process.execPath,
  [electronBuilder, '--win', 'dir', '--config', 'electron-builder.yml'],
  {
    cwd: desktopRoot,
    env: environment,
    stdio: 'inherit',
  },
);

if (result.error) {
  throw result.error;
}

process.exit(result.status ?? 1);
