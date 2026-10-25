// Typed surface of `core-binding.mjs`.
//
// The rules themselves are plain JavaScript because a build script, a read-only
// preflight and the Electron Main process all load them. This declaration is what
// lets the typed consumers use them without turning on `allowJs` for the whole
// workspace. It sits beside the runtime module so the compiled Main imports a
// sibling rather than a path back into the repository.

export declare const PROGRAM_DATA_BASE: readonly ['Aura', 'R06-Spike'];

export declare const normaliseWin32Path: (value: string) => string;
export declare const installationBase: (programData: string) => string;
export declare const publishedCorePath: (programData: string, installationId: string) => string;

export type InstallationStatus =
  | 'usable'
  | 'not_an_installation'
  | 'invalid'
  | 'unreadable'
  | 'published_path_mismatch'
  | 'published_core_missing'
  | 'published_hash_mismatch';

export interface InstallationFact {
  installationId: string;
  root: string;
  status: InstallationStatus;
  publishedCore?: string;
  declaredPath?: string;
  declaredSha256?: string | null;
  /** Declared hash of the other execution images, keyed by component. */
  declaredExecutionSha256?: Readonly<Record<string, string | null>>;
  actualSha256?: string;
  revision?: string;
}

export interface InstallationScan {
  base: string;
  unreadable: boolean;
  installations: InstallationFact[];
}

export declare function readExecutionInstallations(programData: string): Promise<InstallationScan>;

export type CoreResolutionStatus =
  | 'selected'
  | 'expected_version_unknown'
  | 'installation_missing'
  | 'installation_store_unreadable'
  | 'not_published_for_this_version'
  | 'published_path_mismatch'
  | 'ambiguous'
  | 'installation_id_mismatch'
  | 'execution_version_mismatch';

export interface CoreResolution {
  status: CoreResolutionStatus;
  path?: string;
  installationId?: string;
  declaredSha256?: string;
  /** Which expected execution components the installation does not declare. */
  mismatchedComponents?: readonly string[];
  unknownComponents?: readonly string[];
  installations: InstallationFact[];
}

export declare function resolvePublishedCore(options: {
  programData: string;
  expectedSha256: string;
  installationId?: string | undefined;
  /** When supplied, requires valid core, runner and powershell hashes. */
  expectedExecutionSha256?: Readonly<Record<string, string | null>> | undefined;
}): Promise<CoreResolution>;

export interface CoreBinding {
  state: 'bound' | 'unbound';
  gap: CoreResolutionStatus | null;
  corePath: string | null;
  installationId: string | null;
  declaredSha256: string | null;
}

export declare function describeCoreBinding(resolution: CoreResolution): CoreBinding;

export declare const CORE_BINDING_GAP_TEXT: Readonly<Record<CoreResolutionStatus, string>>;
