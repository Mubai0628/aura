// Typed facade over the one published-Core rule set.
//
// The rules themselves live in `./core-binding.mjs`, beside this file, because the
// ordinary `corepack pnpm dev` entry — a Node process, not this Electron Main
// bundle — has to answer exactly the same question about exactly the same
// installation, and two copies of that answer is how a developer ends up running
// a Core the packaged product would have refused. The module is a sibling rather
// than a path back into the repository so it is emitted next to the compiled Main
// and therefore resolves in a standalone candidate package too. This file adds only
// the typing and the packaged-spawn policy.
import {
  CORE_BINDING_GAP_TEXT,
  installationBase,
  normaliseWin32Path,
  publishedCorePath,
  readExecutionInstallations,
  resolvePublishedCore,
} from './core-binding.mjs';

export { CORE_BINDING_GAP_TEXT, installationBase, normaliseWin32Path, publishedCorePath };

/** Alias kept for the one name the packaged resolver has always exported. */
export const win32FilePath = normaliseWin32Path;

export type PublishedCoreResolution =
  | {
      status: 'selected';
      path: string;
      installationId: string;
      declaredSha256: string;
      installations: InstallationFact[];
    }
  | {
      status: PublishedCoreRefusal;
      /**
       * Which expected execution components the installation does not declare. Empty
       * for a refusal that is not about the execution set.
       */
      mismatchedComponents: readonly string[];
      unknownComponents?: readonly string[];
      installations: InstallationFact[];
    };

export type PublishedCoreRefusal =
  | 'expected_version_unknown'
  | 'installation_missing'
  | 'installation_store_unreadable'
  | 'not_published_for_this_version'
  | 'published_path_mismatch'
  | 'ambiguous'
  | 'installation_id_mismatch'
  | 'execution_version_mismatch';

export interface InstallationFact {
  installationId: string;
  root: string;
  status: string;
  publishedCore?: string;
  declaredPath?: string;
  declaredSha256?: string | null;
  actualSha256?: string;
}

export type PackagedCoreSpawnEnv = NodeJS.ProcessEnv | Record<string, string | undefined>;

/**
 * Accepts exactly the shape the shared rule set returns, so a Main process built
 * against a newer revision of the rules cannot silently accept a status this
 * build does not know how to handle.
 */
const narrow = (value: unknown): PublishedCoreResolution => {
  if (typeof value !== 'object' || value === null) {
    return { status: 'installation_store_unreadable', mismatchedComponents: [], installations: [] };
  }
  const record = value as Record<string, unknown>;
  const installations = Array.isArray(record.installations)
    ? (record.installations as InstallationFact[])
    : [];
  if (record.status === 'selected') {
    if (typeof record.path !== 'string' || typeof record.installationId !== 'string') {
      return {
        status: 'installation_store_unreadable',
        mismatchedComponents: [],
        installations,
      };
    }
    return {
      status: 'selected',
      path: record.path,
      installationId: record.installationId,
      declaredSha256: String(record.declaredSha256 ?? ''),
      installations,
    };
  }
  const known: PublishedCoreRefusal[] = [
    'expected_version_unknown',
    'installation_missing',
    'installation_store_unreadable',
    'not_published_for_this_version',
    'published_path_mismatch',
    'ambiguous',
    'installation_id_mismatch',
    'execution_version_mismatch',
  ];
  const status = known.find((candidate) => candidate === record.status);
  const mismatchedComponents = Array.isArray(record.mismatchedComponents)
    ? (record.mismatchedComponents as string[])
    : [];
  return status === undefined
    ? { status: 'installation_store_unreadable', mismatchedComponents: [], installations }
    : {
        status,
        mismatchedComponents,
        installations,
        unknownComponents: Array.isArray(record.unknownComponents)
          ? (record.unknownComponents as string[])
          : [],
      };
};

export const readPublishedInstallations = async (
  programData: string,
): Promise<{ installations: InstallationFact[]; unreadable: boolean }> => {
  const scan = (await readExecutionInstallations(programData)) as {
    installations: InstallationFact[];
    unreadable: boolean;
  };
  return scan;
};

/**
 * Whether the packaged spawn must be bound to a published Core.
 *
 * Directed and matrix acceptance runs already promise that the Core under test is
 * the published one, so for them a fallback to the bundle copy would silently
 * invalidate the run.
 */
export const requiresBoundPublishedCore = (env: PackagedCoreSpawnEnv): boolean =>
  env.AURA_ACCEPTANCE_DIRECTED === '1' ||
  (typeof env.AURA_ACCEPTANCE_MATRIX === 'string' && env.AURA_ACCEPTANCE_MATRIX.length > 0);

export const selectPackagedCoreExecutable = (options: {
  published: PublishedCoreResolution | null;
  bundleCore: string;
  env?: PackagedCoreSpawnEnv;
}): string => {
  if (options.published?.status === 'selected') {
    return options.published.path;
  }
  const env = options.env ?? process.env;
  if (requiresBoundPublishedCore(env)) {
    const reason = options.published?.status ?? 'installation_missing';
    throw new Error(
      `bound execution Core spawn refused: published Core ${reason}; bundle fallback is not allowed`,
    );
  }
  return options.bundleCore;
};

/**
 * The published Core this Main process may run, judged against this build's version.
 *
 * `expectedExecutionSha256` is forwarded rather than swallowed: the shared rule set
 * refuses an installation whose Core matches but whose declared execution images are
 * not this build's, and a facade that dropped the expectation would turn that refusal
 * back into a selection. The development preflight and this process therefore ask the
 * same question through the same entry.
 */
export const resolvePublishedCoreExecutable = async (options: {
  programData: string;
  expectedSha256: string;
  installationId?: string;
  expectedExecutionSha256?: Readonly<Record<string, string | null>>;
}): Promise<PublishedCoreResolution> =>
  narrow(
    await resolvePublishedCore({
      programData: options.programData,
      expectedSha256: options.expectedSha256,
      installationId: options.installationId,
      expectedExecutionSha256: options.expectedExecutionSha256,
    }),
  );
