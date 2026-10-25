import { selectSnapshotRevision } from '../projection-snapshot-order.js';

/** What tells one projection's handling apart; everything else is shared. */
export interface ProjectionTrackSpec<Change> {
  /** The start of the error thrown for a revision that is not a safe integer. */
  readonly integerLabel: string;
  /** The label `selectSnapshotRevision` reports a backwards snapshot under. */
  readonly snapshotLabel: string;
  /** The log event for an increment that skipped revisions. */
  readonly gapEvent: string;
  /** The projection's name in the log of a failed gap refresh. */
  readonly refreshName: string;
  /** Whether two payloads say the same, so an unchanged snapshot is not republished. */
  readonly samePayload: (left: Change, right: Change) => boolean;
}

/** What a projection needs from the supervisor that owns it. */
export interface ProjectionTrackHost<Change> {
  /** The Core instance increments must come from. */
  coreRuntimeInstanceId(): string;
  log(event: string, fields: Record<string, unknown>): Promise<void>;
  /** Reads Core's snapshot of this projection and applies it. */
  refresh(): Promise<Change>;
  /** Gives a failed gap refresh an owner (the supervisor logs it). */
  logRefreshFailure(projection: string, error: unknown): Promise<void>;
}

/**
 * One projection Main relays from Core.
 *
 *Core numbers its own revisions (`sourceRevision`); Main republishes every accepted change with a
 * revision of its own that restarts at 1 when the Core instance changes. An increment that skips a
 * revision, or comes from another Core instance, is not applied: one snapshot read reconciles it,
 * and a snapshot never moves the source revision backwards. Listeners receive a copy.
 */
export class RevisionedProjection<
  Change extends { revision: string; runtime_instance_id: string },
> {
  readonly #spec: ProjectionTrackSpec<Change>;
  readonly #host: ProjectionTrackHost<Change>;
  readonly #listeners = new Set<(change: Change) => void>();
  #current: Change;
  #sourceRevision: number | undefined;
  #refreshing = false;

  constructor(
    spec: ProjectionTrackSpec<Change>,
    host: ProjectionTrackHost<Change>,
    initial: Change,
  ) {
    this.#spec = spec;
    this.#host = host;
    this.#current = initial;
  }

  /** The current change, for the supervisor's own reads; listeners and callers get `snapshot`. */
  get current(): Change {
    return this.#current;
  }

  get snapshot(): Change {
    return structuredClone(this.#current);
  }

  /** Core's revision of the last accepted change, or `undefined` before the first snapshot. */
  get sourceRevision(): number | undefined {
    return this.#sourceRevision;
  }

  /** A new Core instance starts its revisions again. */
  resetSourceRevision(): void {
    this.#sourceRevision = undefined;
  }

  subscribe(listener: (change: Change) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  /** Make `change` current and tell every listener. */
  emit(change: Change): void {
    this.#current = change;
    for (const listener of this.#listeners) listener(this.snapshot);
  }

  applyIncrement(change: Change): void {
    if (change.runtime_instance_id !== this.#host.coreRuntimeInstanceId()) {
      void this.refreshAfterGap();
      return;
    }
    const incomingRevision = Number(change.revision);
    if (!Number.isSafeInteger(incomingRevision)) {
      throw new Error(`${this.#spec.integerLabel} revision was not a safe integer`);
    }
    const sourceRevision = this.#sourceRevision;
    if (sourceRevision === undefined) {
      void this.refreshAfterGap();
      return;
    }
    if (incomingRevision <= sourceRevision) return;
    if (incomingRevision !== sourceRevision + 1) {
      void this.#host.log(this.#spec.gapEvent, {
        expectedRevision: sourceRevision + 1,
        incomingRevision,
      });
      void this.refreshAfterGap();
      return;
    }
    this.#sourceRevision = incomingRevision;
    this.#publish(change);
  }

  applySnapshot(snapshot: Change, revisionAtRequest: number | undefined): void {
    const sourceRevision = selectSnapshotRevision(
      snapshot.revision,
      revisionAtRequest,
      this.#sourceRevision,
      this.#spec.snapshotLabel,
    );
    if (sourceRevision === undefined) return;
    const sourceUnchanged = sourceRevision === this.#sourceRevision;
    this.#sourceRevision = sourceRevision;
    if (
      sourceUnchanged &&
      snapshot.runtime_instance_id === this.#current.runtime_instance_id &&
      this.#spec.samePayload(snapshot, this.#current)
    ) {
      return;
    }
    this.#publish(snapshot);
  }

  /** One snapshot read at a time reconciles a gap; a failed read is logged, never thrown. */
  async refreshAfterGap(): Promise<void> {
    if (this.#refreshing) return;
    this.#refreshing = true;
    try {
      await this.#host.refresh();
    } catch (error) {
      await this.#host.logRefreshFailure(this.#spec.refreshName, error);
    } finally {
      this.#refreshing = false;
    }
  }

  #publish(change: Change): void {
    const sameRuntime = change.runtime_instance_id === this.#current.runtime_instance_id;
    this.emit({
      ...change,
      revision: String(sameRuntime ? Number(this.#current.revision) + 1 : 1),
    });
  }
}
