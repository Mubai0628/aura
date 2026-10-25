// Gives a release's programs Aura's icon and version information, with the rcedit that
// electron-winstaller carries in node_modules (no download).
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';

const REPOSITORY = resolve(import.meta.dirname, '../..');
export const ICON = join(REPOSITORY, 'resources/app-icon/aura.ico');

export function findRcedit() {
  const store = join(REPOSITORY, 'node_modules/.pnpm');
  const candidates = existsSync(store)
    ? readdirSync(store)
        .filter((name) => name.startsWith('electron-winstaller@'))
        .map((name) => join(store, name, 'node_modules/electron-winstaller/vendor/rcedit.exe'))
    : [];
  const found = candidates.find((path) => existsSync(path));
  if (!found) throw new Error('rcedit was not found: run corepack pnpm install first');
  return found;
}

/**
 * Sets the icon, and for a program that already carries version information (the Electron
 * desktop), the version strings Windows shows in its properties. Programs built without version
 * information get the icon only: rcedit can change that resource but not create it.
 */
export function brand(program, { version, description, iconOnly = false }, rcedit = findRcedit()) {
  const versionArguments = iconOnly
    ? []
    : [
        '--set-file-version',
        version,
        '--set-product-version',
        version,
        '--set-version-string',
        'ProductName',
        'Aura',
        '--set-version-string',
        'FileDescription',
        description,
        '--set-version-string',
        'CompanyName',
        'Aura Project',
        '--set-version-string',
        'LegalCopyright',
        'Aura Project',
      ];
  execFileSync(rcedit, [program, '--set-icon', ICON, ...versionArguments]);
}
