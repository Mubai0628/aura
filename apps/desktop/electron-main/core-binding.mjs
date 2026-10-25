// Which Core an Aura entry point may actually run, and why.
//
// This is the one place both the packaged desktop and the ordinary `corepack pnpm
// dev` entry ask that question. It is deliberately plain JavaScript with no
// dependencies so a build script, a read-only preflight and the Electron Main
// process can all load it without a build step or a second copy of the rules.
//
// It lives inside `electron-main/` rather than in the repository's `scripts/`
// directory because Main has to **ship** it. `tsc` rewrites a relative import only
// as far as the source tree is concerned: a source that reached back to the
// repository root resolved inside the checkout and then resolved to a path that
// does not exist once Main was compiled and packaged. Keeping the module beside its
// consumer means the compiled `dist/electron-main/core-binding.mjs` resolves in the
// development directory and in a standalone candidate package alike, which is the
// property the previous layout could not have had.
//
// The rule it implements is the same for every entry:
//
// * A published execution installation is bound to one Core file object. Selecting
//   any other Core leaves readiness failing, no matter how similar the file is.
// * A published Core is only usable when the manifest under examination declares
//   exactly that hash and the file on disk is that hash. A copy with the same
//   bytes at a different path is a different file object, and a declared path that
//   is not the install-root `aura-core.exe` is a Desktop-bound Core, not this one.
// * Two usable installations are ambiguous. Picking one would be a guess, so the
//   answer is `ambiguous` and the caller has to resolve it.
//
// An answer is never "close enough": every non-`selected` status is a refusal with
// a stable machine-readable name.

import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { lstat, readdir, readFile } from 'node:fs/promises';
import { join, win32 } from 'node:path';

/** The ProgramData base every execution installation lives under. */
export const PROGRAM_DATA_BASE = Object.freeze(['Aura', 'R06-Spike']);

const INSTALL_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu;
const SHA256 = /^[0-9a-f]{64}$/iu;

/** Normalizes a Windows path the way the Rust binding compares it. */
export const normaliseWin32Path = (value) =>
  win32
    .normalize(value.replace(/^\\\\\?\\/u, ''))
    .replace(/\\+$/u, '')
    .toLowerCase();

export const installationBase = (programData) =>
  programData ? join(programData, ...PROGRAM_DATA_BASE) : '';

/** The Core path an installation of `installationId` publishes. */
export const publishedCorePath = (programData, installationId) =>
  installationBase(programData)
    ? join(installationBase(programData), installationId, 'aura-core.exe')
    : '';

async function regularFile(path) {
  try {
    const info = await lstat(path);
    return !info.isSymbolicLink() && info.isFile();
  } catch {
    return false;
  }
}

async function directory(path) {
  try {
    const info = await lstat(path);
    return !info.isSymbolicLink() && info.isDirectory();
  } catch {
    return false;
  }
}

async function sha256File(path) {
  const hash = createHash('sha256');
  for await (const bytes of createReadStream(path)) hash.update(bytes);
  return hash.digest('hex');
}

function declaredHash(manifest) {
  const value = manifest?.core_image?.sha256;
  if (typeof value === 'string' && SHA256.test(value)) return value.toLowerCase();
  if (value && typeof value.hex === 'string' && SHA256.test(value.hex)) {
    return value.hex.toLowerCase();
  }
  return null;
}

/** The declared hash of one named installation image, or `null`. */
function imageHash(image) {
  const value = image?.sha256;
  if (typeof value === 'string' && SHA256.test(value)) return value.toLowerCase();
  if (value && typeof value.hex === 'string' && SHA256.test(value.hex)) {
    return value.hex.toLowerCase();
  }
  return null;
}

/**
 * Every execution installation under `programData`, with the facts needed to
 * judge it. Read-only: nothing is created, repaired or written.
 */
export async function readExecutionInstallations(programData) {
  const base = installationBase(programData);
  const result = { base, unreadable: false, installations: [] };
  if (!base || !(await directory(base))) return result;
  let entries;
  try {
    entries = await readdir(base, { withFileTypes: true });
  } catch {
    result.unreadable = true;
    return result;
  }
  for (const entry of entries) {
    if (!entry.isDirectory() || entry.isSymbolicLink()) continue;
    const root = join(base, entry.name);
    const manifestPath = join(root, 'execution.json');
    const core = join(root, 'aura-core.exe');
    if (!INSTALL_ID.test(entry.name)) {
      result.installations.push({
        installationId: entry.name,
        root,
        status: 'not_an_installation',
      });
      continue;
    }
    if (!(await directory(root)) || !(await regularFile(manifestPath))) {
      result.installations.push({
        installationId: entry.name,
        root,
        status: 'invalid',
      });
      continue;
    }
    let manifest;
    try {
      manifest = JSON.parse((await readFile(manifestPath)).toString('utf8'));
    } catch {
      result.installations.push({
        installationId: entry.name,
        root,
        status: 'unreadable',
      });
      continue;
    }
    const expected = declaredHash(manifest);
    const declaredPath =
      typeof manifest?.core_image?.path === 'string' ? manifest.core_image.path : '';
    const declared = {
      installationId: entry.name,
      root,
      publishedCore: core,
      declaredPath,
      declaredSha256: expected,
      // The rest of the execution set. A Core-only comparison cannot notice a
      // runner that was replaced while Core stayed byte-identical, so the declared
      // hashes of every execution object are read too.
      declaredExecutionSha256: {
        runner: imageHash(manifest?.runner_image),
        powershell: imageHash(manifest?.powershell_image),
      },
      revision:
        typeof manifest?.revision === 'string'
          ? manifest.revision
          : String(manifest?.revision ?? ''),
    };
    if (
      manifest?.installation_id !== entry.name ||
      expected === null ||
      declaredPath.length === 0
    ) {
      result.installations.push({ ...declared, status: 'invalid' });
      continue;
    }
    if (normaliseWin32Path(declaredPath) !== normaliseWin32Path(core)) {
      // The install root is the only Core this installation may run. A manifest
      // naming a build output or a Desktop folder is a Desktop-bound Core.
      result.installations.push({ ...declared, status: 'published_path_mismatch' });
      continue;
    }
    if (!(await regularFile(core))) {
      result.installations.push({ ...declared, status: 'published_core_missing' });
      continue;
    }
    const actual = await sha256File(core);
    if (actual !== expected) {
      result.installations.push({
        ...declared,
        status: 'published_hash_mismatch',
        actualSha256: actual,
      });
      continue;
    }
    result.installations.push({ ...declared, status: 'usable', actualSha256: actual });
  }
  result.installations.sort((a, b) => (a.installationId < b.installationId ? -1 : 1));
  return result;
}

/**
 * The published Core an entry point may run, judged against `expectedSha256`.
 *
 * `expectedSha256` is the hash of the Core this run is supposed to be executing.
 * Passing it is what turns "an installation exists" into "this installation is the
 * version this run is bound to".
 *
 * Omission supports Core-only inspection. Production entries always provide the
 * complete Core/Runner/PowerShell set, with missing values explicitly null.
 * Core alone is a necessary condition, not a
 * sufficient one: the installation's runner is a separate file object that can be
 * replaced without touching Core, so an installation whose Core matches can still be
 * running an execution version this source tree did not produce.
 */
export async function resolvePublishedCore({
  programData,
  expectedSha256,
  installationId,
  expectedExecutionSha256,
}) {
  const expected = typeof expectedSha256 === 'string' ? expectedSha256.toLowerCase() : '';
  const unknown = new Set(SHA256.test(expected) ? [] : ['core']);
  // Omission is only the legacy Core-only inspection API. Production always
  // supplies the complete set, including nulls for missing evidence.
  if (expectedExecutionSha256 !== undefined) {
    for (const kind of ['core', 'runner', 'powershell']) {
      const value = expectedExecutionSha256?.[kind];
      if (
        typeof value !== 'string' ||
        !SHA256.test(value.toLowerCase()) ||
        (kind === 'core' && value.toLowerCase() !== expected)
      )
        unknown.add(kind);
    }
  }
  if (unknown.size)
    return {
      status: 'expected_version_unknown',
      unknownComponents: [...unknown].sort(),
      installations: [],
    };
  const scan = await readExecutionInstallations(programData);
  if (scan.unreadable) return { status: 'installation_store_unreadable', installations: [] };
  if (scan.installations.length === 0) return { status: 'installation_missing', installations: [] };
  const wanted = typeof installationId === 'string' ? installationId.toLowerCase() : undefined;
  const selected = scan.installations.filter(
    (item) => item.status === 'usable' && item.declaredSha256 === expected,
  );
  if (selected.length > 1) {
    return {
      status: 'ambiguous',
      installations: selected.map((item) => summarise(item)),
    };
  }
  if (selected.length === 0) {
    // Distinguish "nothing is published for this version" from "this version is
    // published but bound somewhere else". The second is a different repair.
    const forVersion = scan.installations.filter((item) => item.declaredSha256 === expected);
    if (forVersion.some((item) => item.status === 'published_path_mismatch')) {
      return {
        status: 'published_path_mismatch',
        installations: forVersion.map((item) => summarise(item)),
      };
    }
    return {
      status: 'not_published_for_this_version',
      installations: scan.installations.map((item) => summarise(item)),
    };
  }
  const found = selected[0];
  if (wanted !== undefined && found.installationId.toLowerCase() !== wanted) {
    return { status: 'installation_id_mismatch', installations: [summarise(found)] };
  }
  const executionMismatch = mismatchInExecutionSet(found, expectedExecutionSha256);
  if (executionMismatch !== null) {
    return {
      status: 'execution_version_mismatch',
      mismatchedComponents: executionMismatch,
      installations: [summarise(found)],
    };
  }
  return {
    status: 'selected',
    path: found.publishedCore,
    installationId: found.installationId,
    declaredSha256: found.declaredSha256,
    installations: [summarise(found)],
  };
}

/**
 * Which expected execution components the installation does not declare.
 *
 * Completeness has already been checked. A missing installation declaration is a
 * mismatch; a missing build expectation is refused earlier as unknown.
 */
function mismatchInExecutionSet(installation, expectedExecutionSha256) {
  if (expectedExecutionSha256 === undefined || expectedExecutionSha256 === null) return null;
  const declared = { ...installation.declaredExecutionSha256, core: installation.declaredSha256 };
  const mismatched = [];
  for (const [kind, value] of Object.entries(expectedExecutionSha256)) {
    const wanted = typeof value === 'string' ? value.toLowerCase() : null;
    if (wanted === null || !SHA256.test(wanted)) continue;
    if (declared[kind] !== wanted) mismatched.push(kind);
  }
  return mismatched.length === 0 ? null : mismatched.sort();
}

function summarise(item) {
  return {
    installationId: item.installationId,
    root: item.root,
    status: item.status,
    publishedCore: item.publishedCore,
    declaredPath: item.declaredPath,
    declaredSha256: item.declaredSha256,
    ...(item.actualSha256 === undefined ? {} : { actualSha256: item.actualSha256 }),
  };
}

/**
 * The binding an entry point is in, in the terms a person has to act on.
 *
 * `bound` means the published Core this run must execute was found and verified.
 * `unbound` means it was not, and `gap` says which fact is missing. Both carry the
 * bounded evidence the caller should log.
 */
export function describeCoreBinding(resolution) {
  if (resolution.status === 'selected') {
    return {
      state: 'bound',
      gap: null,
      corePath: resolution.path,
      installationId: resolution.installationId,
      declaredSha256: resolution.declaredSha256,
    };
  }
  return {
    state: 'unbound',
    gap: resolution.status,
    corePath: null,
    installationId: null,
    declaredSha256: null,
  };
}

export const CORE_BINDING_GAP_TEXT = Object.freeze({
  expected_version_unknown:
    '本次运行的 Core、Runner 或 PowerShell 版本资料缺失、损坏或不一致，不能确认已发布对象匹配。',
  installation_missing: '本机没有已发布的执行安装。受限 Shell 未准备。',
  installation_store_unreadable: '执行安装目录不可读。受限 Shell 未准备。',
  not_published_for_this_version: '已发布的执行安装不是本次源码构建的版本。受限 Shell 未准备。',
  published_path_mismatch:
    '安装清单绑定的 Core 路径不是该安装目录下的 aura-core.exe，是另一入口绑定的对象。',
  ambiguous: '存在多个与本次版本一致的已发布对象，无法确定应使用哪一个。',
  installation_id_mismatch: '找到的已发布对象与指定的安装 id 不一致。',
  execution_version_mismatch:
    '已发布安装的 Core 与本次构建一致，但执行组件（command runner 等）不是本次构建的版本；只比较 Core 哈希不足以证明整套执行版本一致。',
});
