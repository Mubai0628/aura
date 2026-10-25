import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { useEffect, useSyncExternalStore } from 'react';

import type { CoreHealthProjectionChange } from '../../shared/aura-api';

export interface CoreHealthViewState {
  error?: IpcErrorEnvelope;
  loading: boolean;
  projection?: CoreHealthProjectionChange;
}

const INITIAL_STATE: CoreHealthViewState = { loading: true };

class CoreHealthStore {
  readonly #listeners = new Set<() => void>();
  #queuedChange: CoreHealthProjectionChange | undefined;
  #snapshotEpoch = 0;
  #snapshotRequest: Promise<void> | undefined;
  #started = false;
  #state: CoreHealthViewState = INITIAL_STATE;
  #unsubscribe: (() => void) | undefined;

  getSnapshot = (): CoreHealthViewState => this.#state;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  // One store serves every reader of the window; it listens while any of them is mounted.
  #readers = 0;

  start(): void {
    this.#readers += 1;
    if (this.#started) return;
    this.#started = true;
    this.#unsubscribe = window.aura.v1.onCoreHealth((change) => this.#receiveIncrement(change));
    void this.#requestSnapshot();
  }

  stop(): void {
    this.#readers = Math.max(0, this.#readers - 1);
    if (this.#readers > 0) return;
    this.#unsubscribe?.();
    this.#unsubscribe = undefined;
    this.#started = false;
  }

  resetForTests(): void {
    this.#readers = 0;
    this.stop();
    this.#snapshotEpoch += 1;
    this.#queuedChange = undefined;
    this.#snapshotRequest = undefined;
    this.#state = INITIAL_STATE;
  }

  #receiveIncrement(change: CoreHealthProjectionChange): void {
    const current = this.#state.projection;
    if (current === undefined) {
      this.#queuedChange = change;
      void this.#requestSnapshot();
      return;
    }
    if (change.runtime_instance_id !== current.runtime_instance_id) {
      this.#queuedChange = undefined;
      this.#setState({ loading: true });
      void this.#requestSnapshot();
      return;
    }
    const incomingRevision = Number(change.revision);
    const currentRevision = Number(current.revision);
    if (incomingRevision <= currentRevision) return;
    if (incomingRevision !== currentRevision + 1) {
      this.#queuedChange = undefined;
      this.#setState({ loading: true });
      void this.#requestSnapshot();
      return;
    }
    this.#setState({ loading: false, projection: change });
  }

  #requestSnapshot(): Promise<void> {
    if (this.#snapshotRequest !== undefined) return this.#snapshotRequest;
    const epoch = ++this.#snapshotEpoch;
    this.#snapshotRequest = window.aura.v1
      .getCoreHealth()
      .then((result) => {
        if (epoch !== this.#snapshotEpoch) return;
        if (!result.ok) {
          this.#setState({ error: result.error, loading: false });
          return;
        }
        this.#setState({ loading: false, projection: result.value });
        const queued = this.#queuedChange;
        this.#queuedChange = undefined;
        if (queued !== undefined) this.#receiveIncrement(queued);
      })
      .catch((error: unknown) => {
        if (epoch !== this.#snapshotEpoch) return;
        this.#setState({
          error: {
            category: 'transport',
            code: 'renderer.snapshot_failed',
            correlation_id: 'cor_00000000000000000000000000',
            details: {},
            message: error instanceof Error ? error.message : String(error),
            retryable: true,
            terminal: 'transport_lost',
          },
          loading: false,
        });
      })
      .finally(() => {
        if (epoch === this.#snapshotEpoch) this.#snapshotRequest = undefined;
      });
    return this.#snapshotRequest;
  }

  #setState(state: CoreHealthViewState): void {
    this.#state = state;
    for (const listener of this.#listeners) listener();
  }
}

const coreHealthStore = new CoreHealthStore();

// Non-hook reader for code paths that must gate on the latest Core health outside
// React's render cycle, e.g. deciding whether an IPC failure means "Core has not
// finished capability negotiation yet" instead of a genuine error.
export const readCoreHealthSnapshot = (): CoreHealthViewState => coreHealthStore.getSnapshot();

export const resetCoreHealthStoreForTests = (): void => {
  coreHealthStore.resetForTests();
};

export const useCoreHealth = (): CoreHealthViewState => {
  useEffect(() => {
    coreHealthStore.start();
    return () => coreHealthStore.stop();
  }, []);
  return useSyncExternalStore(
    coreHealthStore.subscribe,
    coreHealthStore.getSnapshot,
    coreHealthStore.getSnapshot,
  );
};
