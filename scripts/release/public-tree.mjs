// What the public repository carries from a commit of this one: the source, the build and test
// tooling, the resources and fixtures, and the public documents. Development records stay here:
// planning documents and reports, the per-stage contract ledgers and stage gates, host-preparation
// and test-machine scripts. Files under public/ are placed over the tree (README, AGENTS.md, the
// changelog, the security policy, the CI workflow).

export const OVERLAY = 'public/';

const ROOT_FILES = new Set([
  '.cargo/config.toml',
  '.editorconfig',
  '.gitattributes',
  '.gitignore',
  '.node-version',
  '.npmrc',
  '.nvmrc',
  '.prettierignore',
  'Cargo.lock',
  'Cargo.toml',
  'LICENSE',
  'clippy.toml',
  'eslint.config.mjs',
  'package.json',
  'pnpm-lock.yaml',
  'pnpm-workspace.yaml',
  'prettier.config.mjs',
  'rust-toolchain.toml',
  'rustfmt.toml',
  'tsconfig.base.json',
]);

const INCLUDE = [
  /^(crates|apps|packages|xtask|resources|fixtures|runtimes|scripts)\//u,
  /^docs\/(user|dev|examples|reference)\//u,
];

/** Paths inside the included folders that stay in the development repository, with why. */
export const EXCLUDE = [
  // Per-stage contract ledgers and the gates that read them; contract-baseline.mjs replaces them.
  [/^scripts\/architecture\/[^/]*-contract-migration(\.test)?\.mjs$/u, 'stage contract ledger'],
  [
    /^scripts\/architecture\/(check-r0\d-contracts|check-s07-contracts|check-network-policy-contracts|check-contract-migration|snapshot|contract-test-history)\.mjs$/u,
    'stage contract gate',
  ],
  // Host-preparation and test-machine operation. Public are the files a build embeds and the
  // preparation of the pinned PowerShell a release carries.
  [
    /^scripts\/r06\/(?!(egress\/(invoke-probe\.ps1|dns-api\.cs|token-api\.cs|winhttp-api\.cs)|dns-probe-native\.ps1|network-experiment-probe\.ps1|prepare-runtime\.ps1|common\.ps1|runtime-package\.ps1|record-store\.ps1)$)/u,
    'host-preparation script',
  ],
  [/^scripts\/execution\/(?!execution-binding(\.test)?\.mjs$)/u, 'test-machine operation script'],
  [
    /^scripts\/(launch-clean-p05-acceptance|collect-dev-execution-binding-evidence|release-context-binding\.test|execution-image-build\.test)\.mjs$/u,
    'development acceptance helper',
  ],
  [/^apps\/desktop\/vite\.acceptance\.config\.ts$/u, 'development acceptance build'],
  [
    /^apps\/desktop\/tests\/(l13-3-[^/]*\.spec\.ts|l13-3\/)/u,
    'needs a locally built media host and isolated Core',
  ],
];

/** Whether a tracked path of the development repository goes to the public tree. */
export function isPublic(path) {
  if (path.startsWith(OVERLAY)) return false;
  if (!ROOT_FILES.has(path) && !INCLUDE.some((rule) => rule.test(path))) return false;
  return !EXCLUDE.some(([rule]) => rule.test(path));
}

/** Rewrites of exported files, by path; each gets the text and the set of exported paths. */
export const TRANSFORMS = {
  'package.json': (text) => {
    const value = JSON.parse(text);
    for (const name of ['dev:acceptance', 'dev:acceptance:install']) delete value.scripts[name];
    return `${JSON.stringify(value, null, 2)}\n`;
  },
  // The size check reads only the ceilings; files that are not exported have none to keep.
  'scripts/architecture/file-size-baseline.json': (text, exported) => {
    const { metric, ceilings } = JSON.parse(text);
    const kept = Object.fromEntries(
      Object.entries(ceilings).filter(([path]) => exported.has(path)),
    );
    return `${JSON.stringify({ metric, ceilings: kept }, null, 2)}\n`;
  },
};
