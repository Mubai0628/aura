// What the Development Release Bundle of this checkout is bound to.
//
// The file sits beside `core-binding.mjs` in `electron-main` on purpose. The compiled
// Main reads it, `tsc` emits nothing for a hand-written `.mjs`, and only
// `scripts/copy-electron-runtime-modules.mjs` puts it into `dist/electron-main`. A copy
// under `shared/` therefore resolved in the checkout and failed in the packaged
// application — the same assembly break, one directory over.
//
// `scripts/prepare-dev-release.mjs` writes both the Release Bundle manifest the
// Core validates and, next to it, the record of which component hashes it could
// bind to real build outputs. Readers need the second document because the first
// has to keep the exact shape the product compares: a component the checkout has
// not built yet keeps its placeholder digest, and "the manifest still carries a
// placeholder" is not something a reader can detect from the manifest alone.
//
// The record carries two things a reader must not conflate:
//
// * `componentRoot` — where the Release Bundle's own components (Sandbox Host,
//   Egress Proxy, Elevated Helper) are read from. Core needs it because a published
//   Core runs from the execution installation, which publishes execution objects
//   only; its siblings are not the desktop components.
// * `executionComponents` — the expected hash of each execution object. Knowing the
//   Core hash alone cannot detect a rebuilt Runner, because the installation's
//   Runner is a separate file that can be replaced without touching Core.
import { createHash } from 'node:crypto';
import { readFile, stat } from 'node:fs/promises';
import { dirname, join, parse, resolve } from 'node:path';

export const DEVELOPMENT_BINDING_SCHEMA = 'aura-development-component-binding-v1';

const EMPTY = Object.freeze({
  source: 'missing',
  coreSha256: null,
  boundComponents: Object.freeze([]),
  unboundComponents: Object.freeze([]),
  bootComponents: Object.freeze([]),
  missingBootComponents: Object.freeze([]),
  executionComponents: Object.freeze({}),
  unboundExecutionComponents: Object.freeze([]),
  componentRoot: null,
  mainPipeBridgePath: null,
});

/** The Release Bundle components Core resolves by name at startup. */
const BOOT_COMPONENTS = ['sandbox_host', 'egress_proxy', 'elevated_helper'];

/**
 * The repository root above `start`.
 *
 * Found by walking up to the marker this workspace always has, rather than by
 * counting `../` from one module's location: this file is read from the compiled
 * Main, from `scripts/dev-execution-preflight.mjs` and, in a candidate package, from
 * a directory with no repository above it at all. A fixed depth would silently point
 * at the wrong directory for at least one of them — which is the same class of
 * mistake as reaching out of the emitted output.
 */
async function findRepositoryRoot(start) {
  let directory = start;
  for (;;) {
    try {
      if ((await stat(join(directory, 'pnpm-workspace.yaml'))).isFile()) return directory;
    } catch {
      // Keep walking; an unreadable candidate is not the root.
    }
    const parent = dirname(directory);
    if (parent === directory || directory === parse(directory).root) return null;
    directory = parent;
  }
}

const sha256OrNull = (value) =>
  typeof value === 'string' && /^[0-9a-f]{64}$/iu.test(value) ? value.toLowerCase() : null;

/**
 * The binding record for `repositoryRoot`, or a `missing` record.
 *
 * A missing record is not an error: `prepare-dev-release.mjs` may not have run
 * yet, and the honest answer then is "this run has no bound Core version", which
 * is exactly what makes every published object unselectable.
 */
export async function readDevelopmentBinding(repositoryRoot) {
  const found =
    typeof repositoryRoot === 'string' && repositoryRoot.length > 0
      ? repositoryRoot
      : await findRepositoryRoot(import.meta.dirname);
  if (found === null) return { ...EMPTY, path: '' };
  const path = resolve(found, 'target/aura-dev-runtime/development-binding.json');
  let document;
  try {
    document = JSON.parse((await readFile(path)).toString('utf8'));
  } catch {
    return { ...EMPTY, path };
  }
  if (document?.schema !== DEVELOPMENT_BINDING_SCHEMA) return { ...EMPTY, path };
  let manifestCore = null;
  try {
    const manifest = JSON.parse(
      await readFile(resolve(found, 'target/aura-dev-runtime/component-manifest.json'), 'utf8'),
    );
    const core = manifest.components?.find((component) => component.kind === 'core');
    if (
      manifest.bundle_namespace === document.bundle_namespace &&
      core?.content_hash?.algorithm === 'sha256'
    ) {
      manifestCore = sha256OrNull(core.content_hash.hex);
    }
  } catch {
    // A missing or inconsistent manifest cannot certify this execution version.
  }
  const boundComponents = Array.isArray(document.bound_components) ? document.bound_components : [];
  const rawExecution =
    document.execution_components !== null && typeof document.execution_components === 'object'
      ? document.execution_components
      : {};
  const executionComponents = {};
  for (const [kind, value] of Object.entries(rawExecution)) {
    const digest = sha256OrNull(value);
    if (digest !== null) executionComponents[kind] = digest;
  }
  return {
    source: 'development-bundle',
    path,
    coreSha256: manifestCore === sha256OrNull(document.core_sha256) ? manifestCore : null,
    boundComponents,
    unboundComponents: Array.isArray(document.unbound_components)
      ? document.unbound_components
      : [],
    // Whether Core's two sidecars and the elevated helper were actually built.
    // Without them a published Core cannot start even when its own hash matches.
    bootComponents: BOOT_COMPONENTS.filter((kind) => boundComponents.includes(kind)),
    missingBootComponents: BOOT_COMPONENTS.filter((kind) => !boundComponents.includes(kind)),
    executionComponents,
    unboundExecutionComponents: Array.isArray(document.unbound_execution_components)
      ? document.unbound_execution_components
      : [],
    componentRoot: typeof document.component_root === 'string' ? document.component_root : null,
    mainPipeBridgePath:
      typeof document.main_pipe_bridge_path === 'string' ? document.main_pipe_bridge_path : null,
  };
}

/**
 * The execution version a published installation is compared against.
 *
 * The installation manifest declares three images — Core, the command runner and the
 * pinned PowerShell runtime — and each is a separate file object that can be replaced
 * without touching the others. Comparing Core alone therefore answers a weaker question
 * than "is the published installation this build?", so every entry point that asks that
 * question builds its expectation here and hands it to the one resolver.
 *
 * Missing values stay explicit. The resolver refuses an incomplete execution set;
 * unknown is neither a guessed version nor permission to skip the comparison.
 */
export function expectedExecution(binding) {
  const core = sha256OrNull(binding?.coreSha256);
  const expected = {
    core:
      !binding?.unboundExecutionComponents?.includes('core') &&
      core === sha256OrNull(binding?.executionComponents?.core)
        ? core
        : null,
  };
  for (const kind of ['runner', 'powershell']) {
    expected[kind] = binding?.unboundExecutionComponents?.includes(kind)
      ? null
      : sha256OrNull(binding?.executionComponents?.[kind]);
  }
  return expected;
}

/** Generated at release assembly and covered by the sealed bundle inventory. No
 * dependency on a checkout or the candidate's outer managed-runtime directory. */
export async function readPackagedExecution(versionDirectory) {
  try {
    const document = JSON.parse(
      await readFile(resolve(versionDirectory, 'execution-binding.json'), 'utf8'),
    );
    if (document?.schema !== 'aura-release-execution-binding-v1') return expectedExecution(null);
    const expected = expectedExecution({
      coreSha256: document.core_sha256,
      executionComponents: document.execution_components,
    });
    const runner = createHash('sha256')
      .update(await readFile(resolve(versionDirectory, 'aura-command-runner.exe')))
      .digest('hex');
    if (expected.runner !== runner) expected.runner = null;
    return expected;
  } catch {
    return expectedExecution(null);
  }
}
