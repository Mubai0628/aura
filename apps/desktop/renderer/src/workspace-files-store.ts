import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { useEffect, useSyncExternalStore } from 'react';

import type { WorkspaceFilesProjectionChange } from '../../shared/aura-api';

interface WorkspaceFilesViewState {
  error?: IpcErrorEnvelope;
  loading: boolean;
  projection?: WorkspaceFilesProjectionChange;
}

const INITIAL_STATE: WorkspaceFilesViewState = { loading: true };

class WorkspaceFilesStore {
  readonly #listeners = new Set<() => void>();
  #queuedChange: WorkspaceFilesProjectionChange | undefined;
  #snapshotRequest: Promise<void> | undefined;
  #started = false;
  #state: WorkspaceFilesViewState = INITIAL_STATE;
  #unsubscribe: (() => void) | undefined;

  getSnapshot = (): WorkspaceFilesViewState => this.#state;

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
    // A window whose bridge offers no file projection (a page under test) simply shows none.
    if (typeof window.aura?.v1?.onWorkspaceFiles !== 'function') {
      this.#setState({ loading: false });
      return;
    }
    this.#unsubscribe = window.aura.v1.onWorkspaceFiles((change) => this.#receiveIncrement(change));
    void this.#requestSnapshot();
  }

  stop(): void {
    this.#readers = Math.max(0, this.#readers - 1);
    if (this.#readers > 0) return;
    this.#unsubscribe?.();
    this.#unsubscribe = undefined;
    this.#started = false;
  }

  #receiveIncrement(change: WorkspaceFilesProjectionChange): void {
    const current = this.#state.projection;
    if (current === undefined) {
      this.#queuedChange = change;
      void this.#requestSnapshot();
      return;
    }
    if (change.runtime_instance_id !== current.runtime_instance_id) {
      this.#queuedChange = change;
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
    this.#snapshotRequest = window.aura.v1
      .getWorkspaceFiles()
      .then((result) => {
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
        this.#setState({
          error: {
            category: 'transport',
            code: 'renderer.workspace_files_snapshot_failed',
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
        this.#snapshotRequest = undefined;
      });
    return this.#snapshotRequest;
  }

  #setState(state: WorkspaceFilesViewState): void {
    this.#state = state;
    for (const listener of this.#listeners) listener();
  }
}

const workspaceFilesStore = new WorkspaceFilesStore();

export const useWorkspaceFiles = (): WorkspaceFilesViewState => {
  useEffect(() => {
    workspaceFilesStore.start();
    return () => workspaceFilesStore.stop();
  }, []);
  return useSyncExternalStore(
    workspaceFilesStore.subscribe,
    workspaceFilesStore.getSnapshot,
    workspaceFilesStore.getSnapshot,
  );
};
