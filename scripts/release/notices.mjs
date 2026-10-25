// THIRD-PARTY-NOTICES.txt for a release: every third-party Rust crate the shipped programs link
// and every npm package the desktop bundles, each with its license and the license texts it
// carries (one copy of each distinct text), plus the components that bring their own notices.
// scripts/release/assemble.mjs writes it into each release; alone:
//   node scripts/release/notices.mjs <out file>
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const REPOSITORY = resolve(import.meta.dirname, '../..');
/** The programs a release ships; their normal dependencies on Windows are what it links. */
export const SHIPPED = [
  'aura-core',
  'aura-launcher',
  'aura-sandbox-host',
  'aura-egress-proxy',
  'aura-elevated-helper',
  'aura-setup',
];
const LICENSE_FILE = /^(licen[cs]e|copying|notice)/iu;

function licenseTexts(folder) {
  let names;
  try {
    names = readdirSync(folder);
  } catch {
    return [];
  }
  return names
    .filter((name) => LICENSE_FILE.test(name))
    .sort()
    .map((name) => {
      try {
        return readFileSync(join(folder, name), 'utf8').replace(/\r\n/gu, '\n').trim();
      } catch {
        return '';
      }
    })
    .filter(Boolean);
}

export function rustPackages() {
  const metadata = JSON.parse(
    execFileSync(
      'cargo',
      [
        'metadata',
        '--format-version',
        '1',
        '--locked',
        '--filter-platform',
        'x86_64-pc-windows-msvc',
      ],
      { cwd: REPOSITORY, encoding: 'utf8', maxBuffer: 512 * 1024 * 1024 },
    ),
  );
  const packages = new Map(metadata.packages.map((item) => [item.id, item]));
  const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
  const roots = metadata.packages.filter(
    (item) => SHIPPED.includes(item.name) && item.source === null,
  );
  const seen = new Set();
  const stack = roots.map((item) => item.id);
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dependency of nodes.get(id)?.deps ?? []) {
      if (dependency.dep_kinds.some((kind) => kind.kind === null)) stack.push(dependency.pkg);
    }
  }
  return [...seen]
    .map((id) => packages.get(id))
    .filter((item) => item && item.source !== null)
    .map((item) => ({
      name: item.name,
      version: item.version,
      license: item.license ?? (item.license_file ? 'see license file' : 'unknown'),
      texts: licenseTexts(dirname(item.manifest_path)),
    }))
    .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
}

export function npmPackages() {
  const listing = JSON.parse(
    // corepack is a .cmd shim on Windows, which only a command interpreter starts.
    execFileSync(
      process.env.ComSpec ?? 'cmd.exe',
      ['/d', '/c', 'corepack pnpm licenses list --prod --json'],
      { cwd: join(REPOSITORY, 'apps/desktop'), encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 },
    ),
  );
  return Object.values(listing)
    .flat()
    .map((item) => ({
      name: item.name,
      version: item.versions.join(', '),
      license: item.license,
      texts: licenseTexts(item.paths[0]),
    }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

const BUNDLED = [
  'Electron and Chromium: LICENSE.electron.txt and LICENSES.chromium.html in the program folder.',
  'PowerShell 7: LICENSE.txt and ThirdPartyNotices.txt in managed-runtimes\\powershell-7.6.5-win-x64\\payload.',
];

export function notices(groups) {
  const texts = new Map();
  const lines = [
    'THIRD-PARTY NOTICES',
    '',
    'Aura includes the following third-party software. Each is listed with its version and',
    'license; the license texts follow, each distinct text once, with the packages it covers.',
    '',
    ...BUNDLED.map((line) => `* ${line}`),
    '',
  ];
  for (const [title, items] of groups) {
    lines.push(title, '='.repeat(title.length));
    for (const item of items) {
      lines.push(`${item.name} ${item.version} — ${item.license}`);
      for (const text of item.texts) {
        const users = texts.get(text) ?? [];
        users.push(`${item.name} ${item.version}`);
        texts.set(text, users);
      }
    }
    lines.push('');
  }
  lines.push('LICENSE TEXTS', '=============', '');
  for (const [text, users] of texts) {
    lines.push('-'.repeat(72), `Used by: ${users.join(', ')}`, '', text, '');
  }
  return `${lines.join('\n')}\n`;
}

/** The notices of the release this checkout builds, with what they cover. */
export function releaseNotices() {
  const rust = rustPackages();
  const npm = npmPackages();
  const fonts = readdirSync(join(REPOSITORY, 'resources/third-party-notices'))
    .filter((name) => name.endsWith('-LICENSE.txt'))
    .map((name) => ({
      name: name.replace(/-LICENSE\.txt$/u, ''),
      version: '',
      license: 'see text',
      texts: [readFileSync(join(REPOSITORY, 'resources/third-party-notices', name), 'utf8').trim()],
    }));
  const text = notices([
    ['Rust crates', rust],
    ['npm packages', npm],
    ['Fonts and terminal', fonts],
  ]);
  return { text, rust, npm, fonts };
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const out = resolve(process.argv[2] ?? 'THIRD-PARTY-NOTICES.txt');
  const { text, rust, npm, fonts } = releaseNotices();
  writeFileSync(out, text);
  const unknown = rust.filter((item) => item.license === 'unknown').map((item) => item.name);
  console.log(`${out}: ${rust.length} crates, ${npm.length} npm packages, ${fonts.length} more`);
  if (unknown.length) console.log(`no license field: ${unknown.join(', ')}`);
}
