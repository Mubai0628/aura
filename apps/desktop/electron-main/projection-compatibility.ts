import type {
  ProjectionChange,
  ProjectionCompatibilitySnapshotV1,
  ProjectionCompatibilityKey,
  AuxiliaryProjectionKey,
  ProjectionUnsubscribeRequest,
} from '@aura/contracts/generated';
import { parseProjection } from '../shared/projection-validation.js';
import {
  projectionCompatibilityKey,
  projectionKeyFromValue,
} from '../shared/projection-compatibility-validation.js';

/** Only validated route identities survive a malformed payload; invalid bytes never enter a cache. */
export class ProjectionCompatibility {
  #runtime = '';
  #revision = 0n;
  readonly #paused = new Map<string, ProjectionCompatibilityKey>();
  readonly #listeners = new Set<(snapshot: ProjectionCompatibilitySnapshotV1) => void>();

  constructor(private readonly log: (code: string, key?: ProjectionCompatibilityKey) => void) {}

  reset(runtime: string): void {
    this.#runtime = runtime;
    this.#paused.clear();
    this.#publish();
  }

  snapshot(): ProjectionCompatibilitySnapshotV1 {
    return {
      contract_version: 1,
      runtime_instance_id: this.#runtime,
      revision: String(this.#revision),
      paused_projections: structuredClone([...this.#paused.values()]),
    };
  }

  subscribe(listener: (snapshot: ProjectionCompatibilitySnapshotV1) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  readAuxiliary<T>(
    key: AuxiliaryProjectionKey | undefined,
    runtime: string,
    parse: () => T | undefined,
  ): T | undefined {
    if (!key || key.runtime_instance_id !== runtime) {
      this.log('projection.invalid_or_foreign_metadata');
      return undefined;
    }
    const route = projectionCompatibilityKey(key);
    let parsed: T | undefined;
    try {
      parsed = parse();
    } catch {
      /* fixed diagnostic only */
    }
    if (parsed === undefined) {
      this.log('projection.data_format_incompatible', key);
      if (!this.#paused.has(route)) {
        this.#paused.set(route, key);
        this.#publish();
      }
    } else if (this.#paused.delete(route)) this.#publish();
    return parsed;
  }

  read(
    value: unknown,
    runtime: string,
    expectedRoute?: ProjectionUnsubscribeRequest,
    allows: (change: ProjectionChange) => boolean = () => true,
  ): ProjectionChange | undefined {
    const metadata = projectionKeyFromValue(value);
    if (metadata?.runtime_instance_id !== runtime) {
      this.log('projection.invalid_or_foreign_metadata');
      return undefined;
    }
    const expected =
      expectedRoute === undefined
        ? metadata
        : projectionKeyFromValue({ ...expectedRoute, runtime_instance_id: runtime });
    if (expected === undefined) return undefined;
    const route = projectionCompatibilityKey(expected);
    let parsed: ProjectionChange | undefined;
    try {
      parsed = parseProjection(value);
    } catch {
      /* fixed diagnostic below, never payload */
    }
    if (
      parsed === undefined ||
      projectionCompatibilityKey(metadata) !== route ||
      !consumerRevisionIsCompatible(parsed) ||
      !allows(parsed)
    ) {
      this.log('projection.data_format_incompatible', expected);
      if (!this.#paused.has(route)) {
        this.#paused.set(route, expected);
        this.#publish();
      }
      return undefined;
    }
    if (this.#paused.delete(route)) this.#publish();
    return parsed;
  }

  #publish(): void {
    this.#revision += 1n;
    for (const listener of this.#listeners) {
      try {
        listener(this.snapshot());
      } catch {
        this.log('projection.compatibility_delivery_failed');
      }
    }
  }
}

/** These three existing consumers use Number arithmetic. Reject unsupported
 * revisions before they can throw into the authenticated channel owner. */
function consumerRevisionIsCompatible(change: ProjectionChange): boolean {
  return (
    !['core_health', 'workspace_session', 'workspace_files'].includes(change.projection_kind) ||
    Number.isSafeInteger(Number(change.revision))
  );
}
