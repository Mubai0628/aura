// Typed surface of `component-binding.mjs`.

export declare const DEVELOPMENT_BINDING_SCHEMA: 'aura-development-component-binding-v1';

export interface DevelopmentBinding {
  source: 'development-bundle' | 'missing';
  path: string;
  coreSha256: string | null;
  boundComponents: readonly string[];
  unboundComponents: readonly string[];
  /** Release Bundle components Core resolves at startup and that are present. */
  bootComponents: readonly string[];
  /** The same list, for the ones this build did not produce. */
  missingBootComponents: readonly string[];
  /** Expected hash of each execution object the publication copies. */
  executionComponents: Readonly<Record<string, string>>;
  unboundExecutionComponents: readonly string[];
  /** Directory the Release Bundle components are read from. */
  componentRoot: string | null;
  mainPipeBridgePath: string | null;
}

export declare function readDevelopmentBinding(
  repositoryRoot?: string,
): Promise<DevelopmentBinding>;

/**
 * The execution images a published installation must declare to be this build's.
 *
 * Required but unverified components are explicitly null and refuse selection.
 */
export declare function expectedExecution(
  binding: DevelopmentBinding | null | undefined,
): Record<string, string | null>;

export declare function readPackagedExecution(
  versionDirectory: string,
): Promise<Record<string, string | null>>;
