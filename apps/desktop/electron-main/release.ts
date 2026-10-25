import type {
  ActiveInstallState,
  BuildComponent,
  BuildComponentManifest,
  ContentHash,
  ReleaseDescriptor,
} from '@aura/contracts/generated';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { basename, dirname, isAbsolute, relative, resolve } from 'node:path';
import { resolvePublishedCoreExecutable, selectPackagedCoreExecutable } from './published-core.js';
import type { PublishedCoreResolution } from './published-core.js';
import {
  expectedExecution,
  readDevelopmentBinding,
  readPackagedExecution,
} from './component-binding.mjs';

export interface ReleaseContext {
  allowDevMismatch: boolean;
  /**
   * The directory Core reads the Release Bundle's own components from, for a
   * development run. `null` for the packaged product and for a development run whose
   * binding record does not name a complete component set.
   */
  componentRoot: string | null;
  coreComponent: BuildComponent;
  coreExecutable: string;
  desktopComponent: BuildComponent;
  /**
   * How `coreExecutable` was chosen, as a bounded fact rather than a guess.
   * `published` means the Core this run must execute was found published and
   * verified; everything else names the exact gap, so a caller can say that the
   * restricted Shell is not prepared instead of reporting a generic failure.
   */
  coreBinding: CoreBinding;
  manifest: BuildComponentManifest;
  mainPipeBridgeComponent: BuildComponent;
  mainPipeBridgePath: string;
  componentManifestHash: ContentHash;
  manifestPath: string;
  namespaceObjectHash: string;
  releaseDescriptor: ReleaseDescriptor;
  versionDirectory: string;
}

/**
 * The Core-selection outcome every entry point shares.
 *
 * Development and the packaged product ask the same question with the same rule
 * set; only what happens after a refusal differs. A packaged run may keep the
 * bundle copy because that is the shipped product path, while a development run
 * records the gap and leaves restricted execution unready rather than pretending
 * an unpublished binary is the bound one.
 */
export interface CoreBinding {
  state: 'published' | 'refused';
  source: 'published' | 'bundle' | 'debug';
  /** Why the published Core was refused. `null` when one was selected. */
  refusal: string | null;
  /**
   * Which expected execution components the installation did not declare.
   *
   * Non-empty only for `execution_version_mismatch`, so a reader learns *which* part of
   * the execution version differs instead of only that something does.
   */
  mismatchedComponents: readonly string[];
  unknownComponents?: readonly string[];
  installationId: string | null;
}

const BUNDLE_NAMESPACE_PATTERN = /^[a-z0-9][a-z0-9.-]{0,127}$/u;
const DEVELOPMENT_NAMESPACE_PATTERN = /^aura\.desktop\.development\.[0-9a-hjkmnp-tv-z]{26}$/u;
const CURRENT_PROTOCOL_VERSION = { major: 1, minor: 7 } as const;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const parseManifest = (value: unknown): BuildComponentManifest => {
  if (!isRecord(value) || !Array.isArray(value.components)) {
    throw new Error('component-manifest.json has an invalid root shape');
  }
  for (const key of [
    'bundle_namespace',
    'release_bundle_id',
    'product_version',
    'build_commit',
    'target_triple',
  ]) {
    if (typeof value[key] !== 'string') throw new Error(`component manifest is missing ${key}`);
  }
  if (
    !BUNDLE_NAMESPACE_PATTERN.test(value.bundle_namespace as string) ||
    !isSupportedBundleNamespace(value.bundle_namespace as string)
  ) {
    throw new Error('component manifest has an unsupported bundle_namespace');
  }
  if (!['Stable', 'Preview', 'Nightly'].includes(String(value.release_channel))) {
    throw new Error('component manifest has an invalid release_channel');
  }
  if (!isRecord(value.manifest_version)) throw new Error('component manifest has no version');
  for (const component of value.components) {
    if (
      !isRecord(component) ||
      typeof component.component_id !== 'string' ||
      typeof component.kind !== 'string' ||
      typeof component.version !== 'string' ||
      typeof component.relative_path !== 'string' ||
      !isRecord(component.content_hash)
    ) {
      throw new Error('component manifest contains an invalid component');
    }
  }
  return value as unknown as BuildComponentManifest;
};

const parseActiveInstallState = (value: unknown): ActiveInstallState => {
  if (
    !isRecord(value) ||
    typeof value.active !== 'string' ||
    !isRecord(value.release_descriptor) ||
    typeof value.release_descriptor.bundle_namespace !== 'string' ||
    !isRecord(value.release_descriptor.identity) ||
    typeof value.release_descriptor.identity.release_bundle_id !== 'string' ||
    !isRecord(value.release_descriptor.identity.component_manifest_hash) ||
    value.release_descriptor.identity.component_manifest_hash.algorithm !== 'sha256' ||
    typeof value.release_descriptor.identity.component_manifest_hash.hex !== 'string'
  ) {
    throw new Error('active.json has an invalid Release Identity');
  }
  return value as unknown as ActiveInstallState;
};

/**
 * Which entry of active.json the running version is: the active one, whose manifest must be the
 * one its descriptor names, or the waiting one the launcher is trying, which the launcher checked
 * against its own manifest before it started it.
 */
export const runningEntry = (
  active: ActiveInstallState,
  versionName: string,
  manifest: Pick<
    BuildComponentManifest,
    'bundle_namespace' | 'product_version' | 'release_bundle_id'
  >,
  manifestHashHex: string,
): 'active' | 'trial' => {
  const mismatch = new Error('active release identity does not match component-manifest.json');
  if (manifest.product_version !== versionName) throw mismatch;
  if (active.release_descriptor.bundle_namespace !== manifest.bundle_namespace) throw mismatch;
  if (active.active === versionName) {
    if (
      active.release_descriptor.identity.release_bundle_id !== manifest.release_bundle_id ||
      active.release_descriptor.identity.component_manifest_hash.hex !== manifestHashHex
    ) {
      throw mismatch;
    }
    return 'active';
  }
  if (active.pending === versionName) return 'trial';
  throw mismatch;
};

const sha256 = (bytes: Uint8Array): string => createHash('sha256').update(bytes).digest('hex');

const isSupportedBundleNamespace = (value: string): boolean =>
  value === 'aura.desktop.production' ||
  DEVELOPMENT_NAMESPACE_PATTERN.test(value) ||
  value.startsWith('aura.desktop.test.');

const safeReleasePath = (root: string, candidate: string): string => {
  if (isAbsolute(candidate) || candidate.split(/[\\/]/u).includes('..')) {
    throw new Error(`unsafe component path in Release Bundle: ${candidate}`);
  }
  const absolute = resolve(root, candidate);
  const back = relative(root, absolute);
  if (back.startsWith('..') || isAbsolute(back))
    throw new Error('component escaped Release Bundle');
  return absolute;
};

export const loadReleaseContext = async (options: {
  appIsPackaged: boolean;
  currentDirectory: string;
  executablePath: string;
}): Promise<ReleaseContext> => {
  const repositoryRoot = resolve(options.currentDirectory, '../../../..');
  const versionDirectory = options.appIsPackaged ? dirname(options.executablePath) : repositoryRoot;
  const manifestPath = options.appIsPackaged
    ? resolve(versionDirectory, 'component-manifest.json')
    : resolve(repositoryRoot, 'target/aura-dev-runtime/component-manifest.json');
  const manifestBytes = await readFile(manifestPath);
  const manifest = parseManifest(JSON.parse(manifestBytes.toString('utf8')) as unknown);
  const componentManifestHash: ContentHash = {
    algorithm: 'sha256',
    hex: sha256(manifestBytes),
  };
  const releaseDescriptor: ReleaseDescriptor = {
    bundle_namespace: manifest.bundle_namespace,
    identity: {
      release_bundle_id: manifest.release_bundle_id,
      component_manifest_hash: componentManifestHash,
    },
  };
  const namespaceObjectHash = sha256(Buffer.from(manifest.bundle_namespace, 'utf8')).slice(0, 32);
  if (!options.appIsPackaged && !DEVELOPMENT_NAMESPACE_PATTERN.test(manifest.bundle_namespace)) {
    throw new Error('direct development requires the exact per-checkout Development namespace');
  }
  if (options.appIsPackaged) {
    const activePath = resolve(versionDirectory, '..', '..', 'active.json');
    const active = parseActiveInstallState(
      JSON.parse((await readFile(activePath)).toString('utf8')) as unknown,
    );
    runningEntry(active, basename(versionDirectory), manifest, componentManifestHash.hex);
  }
  const desktopComponent = manifest.components.find((component) => component.kind === 'desktop');
  const coreComponent = manifest.components.find((component) => component.kind === 'core');
  const mainPipeBridgeComponent = manifest.components.find(
    (component) => component.kind === 'main_pipe_bridge',
  );
  if (
    desktopComponent === undefined ||
    coreComponent === undefined ||
    mainPipeBridgeComponent === undefined
  ) {
    throw new Error('Release Bundle must contain desktop, Core, and Main Pipe Bridge components');
  }
  for (const kind of [
    'desktop',
    'main_pipe_bridge',
    'core',
    'sandbox_host',
    'egress_proxy',
    'elevated_helper',
  ] as const) {
    const component = manifest.components.find((candidate) => candidate.kind === kind);
    if (component === undefined) throw new Error(`Release Bundle must contain ${kind}`);
    if (
      component.protocol_version === undefined ||
      component.protocol_version === null ||
      component.protocol_version.major !== CURRENT_PROTOCOL_VERSION.major ||
      component.protocol_version.minor > CURRENT_PROTOCOL_VERSION.minor
    ) {
      throw new Error(
        `Release Bundle component protocol is incompatible: ${component.component_id}`,
      );
    }
  }
  const allowDevMismatch =
    !options.appIsPackaged && process.env.AURA_ALLOW_DEV_COMPONENT_MISMATCH === '1';
  if (!options.appIsPackaged && !allowDevMismatch) {
    throw new Error('direct development requires AURA_ALLOW_DEV_COMPONENT_MISMATCH=1');
  }

  const bundleCore = options.appIsPackaged
    ? safeReleasePath(versionDirectory, coreComponent.relative_path)
    : resolve(repositoryRoot, 'target/debug/aura-core.exe');
  // The published-Core lookup is the same lookup in both shapes. Development
  // previously skipped it entirely and always launched `target/debug`, which is
  // exactly the assembly break this closes: the execution installation binds the
  // Core file object the protected publication flow copied into the install root,
  // and a separately built debug binary is a different file object no matter how
  // similar. Asking the one resolver is what makes "the version this run expects"
  // and "the version that is published" the same question.
  //
  // `coreComponent.content_hash.hex` is the version this run expects. For the
  // packaged product it comes from the signed bundle; for development it comes
  // from `target/aura-dev-runtime/component-manifest.json`, which rebuilds the
  // component hashes from the actual build outputs, so a mismatch means the
  // published object is not this source tree's build.
  //
  // Core is a necessary condition, not a sufficient one. The installation manifest
  // also declares the command runner and the pinned PowerShell runtime, and each is a
  // separate file object that can be replaced without touching Core — so an
  // installation whose Core matches can still be running an execution version this
  // build did not produce. Passing only the Core hash is what let this process report
  // `published` for an installation the preflight refuses as
  // `execution_version_mismatch`. The expected set now comes from the same source the
  // preflight reads and goes to the same resolver.
  const published =
    coreComponent.content_hash.algorithm === 'sha256'
      ? await resolvePublishedCoreExecutable({
          programData: process.env.ProgramData ?? process.env.PROGRAMDATA ?? '',
          expectedSha256: coreComponent.content_hash.hex,
          expectedExecutionSha256: await expectedExecutionSet(
            options.appIsPackaged,
            repositoryRoot,
            versionDirectory,
          ),
        })
      : null;
  const coreExecutable = options.appIsPackaged
    ? selectPackagedCoreExecutable({
        published,
        bundleCore,
        env: process.env,
      })
    : published?.status === 'selected'
      ? published.path
      : bundleCore;
  const coreBinding: CoreBinding = resolveCoreBinding(published, options.appIsPackaged);
  // Where Core reads the Release Bundle's own components from.
  //
  // A published Core runs from the execution installation, and that installation
  // deliberately publishes execution objects only — Core, the command runner, the
  // helper and the maintainer. The Sandbox Host, the Egress Proxy and the Elevated
  // Helper are desktop components: they live beside the manifest. Once a development
  // run selects a published Core, resolving them as siblings of `aura-core.exe`
  // finds nothing, so the directory that actually holds them is written down by the
  // development binding rather than guessed from a layout. It is `null` for the
  // packaged product, where Core's own siblings are the bundle.
  const componentRoot = options.appIsPackaged
    ? null
    : await developmentComponentRoot(repositoryRoot);
  const mainPipeBridgePath = options.appIsPackaged
    ? safeReleasePath(versionDirectory, mainPipeBridgeComponent.relative_path)
    : process.env.AURA_E2E === '1'
      ? resolve(repositoryRoot, 'apps/desktop/dist/e2e-native/aura-main-pipe.node')
      : resolve(repositoryRoot, 'apps/desktop/dist/native/aura-main-pipe.node');
  if (options.appIsPackaged) {
    for (const component of manifest.components) {
      if (component.content_hash.algorithm !== 'sha256') {
        throw new Error(`P01 desktop verifier requires SHA-256: ${component.component_id}`);
      }
      const componentPath = safeReleasePath(versionDirectory, component.relative_path);
      const bytes = await readFile(componentPath);
      if (sha256(bytes) !== component.content_hash.hex) {
        throw new Error(`Release Bundle hash mismatch: ${component.component_id}`);
      }
    }
  } else {
    console.warn('[Aura:P01] explicit development component mismatch mode is active');
  }

  return {
    allowDevMismatch,
    componentRoot,
    coreComponent,
    coreExecutable,
    coreBinding,
    desktopComponent,
    manifest,
    mainPipeBridgeComponent,
    mainPipeBridgePath,
    componentManifestHash,
    manifestPath,
    namespaceObjectHash,
    releaseDescriptor,
    versionDirectory,
  };
};

/**
 * The execution version this run expects a published installation to declare.
 *
 * Development reads it from the binding record the build wrote, so the expectation
 * describes files this checkout actually produced. The packaged product reads the
 * execution binding emitted and sealed at assembly, and rechecks its runner file.
 *
 * Missing or malformed expectations refuse selection as unknown. A known version
 * that disagrees with the installation instead reports a version mismatch.
 */
const expectedExecutionSet = async (
  appIsPackaged: boolean,
  repositoryRoot: string,
  versionDirectory: string,
): Promise<Record<string, string | null>> => {
  if (!appIsPackaged) return expectedExecution(await readDevelopmentBinding(repositoryRoot));
  return readPackagedExecution(versionDirectory);
};

/**
 * The directory a development run's Release Bundle components are read from.
 *
 * Read from the binding record the build wrote, so the answer names a directory that
 * was actually produced rather than one reconstructed from the layout. An absent or
 * incomplete record yields `null`, which leaves Core on its previous sibling
 * behaviour: the honest outcome is a missing component reported at startup, not a
 * path this file invented.
 */
const developmentComponentRoot = async (repositoryRoot: string): Promise<string | null> => {
  const binding = await readDevelopmentBinding(repositoryRoot);
  if (binding.source !== 'development-bundle' || binding.componentRoot === null) return null;
  if (binding.missingBootComponents.length > 0) return null;
  return binding.componentRoot;
};

/**
 * Turns a published-Core resolution into the bounded fact every entry logs.
 *
 * A refusal is never silently absorbed: `refusal` carries the machine-readable
 * gap so the readiness projection and the development preflight can both say
 * "restricted Shell is not prepared, for this reason" instead of the generic
 * `runtime_unavailable` that used to hide it.
 */
const resolveCoreBinding = (
  published: PublishedCoreResolution | null,
  appIsPackaged: boolean,
): CoreBinding => {
  if (published?.status === 'selected') {
    return {
      state: 'published',
      source: 'published',
      refusal: null,
      mismatchedComponents: [],
      installationId: published.installationId,
    };
  }
  return {
    state: 'refused',
    source: appIsPackaged ? 'bundle' : 'debug',
    refusal: published?.status ?? 'expected_version_unknown',
    mismatchedComponents: published?.mismatchedComponents ?? [],
    unknownComponents: published?.unknownComponents ?? [],
    installationId: null,
  };
};
