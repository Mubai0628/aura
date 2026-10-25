import { useSyncExternalStore } from 'react';
import type {
  ProjectionCompatibilitySnapshotV1,
  ProjectionUnsubscribeRequest,
} from '@aura/contracts/generated';
import type { ProjectionCompatibilityApiV1 } from '../../shared/projection-compatibility-api';
import {
  isProjectionCompatibilitySnapshot,
  projectionCompatibilityRouteKey,
} from '../../shared/projection-compatibility-validation';

type State = { snapshot?: ProjectionCompatibilitySnapshotV1; activeRoutes: ReadonlySet<string> };
export class ProjectionCompatibilityStore {
  readonly #listeners = new Set<() => void>();
  readonly #routes = new Map<string, number>();
  #state: State = { activeRoutes: new Set() };
  #stop: (() => void) | undefined;

  constructor(private readonly api: () => ProjectionCompatibilityApiV1 | undefined) {}

  getSnapshot = (): State => this.#state;
  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    if (this.#listeners.size === 1) {
      const api = this.api();
      if (api) {
        this.#stop = api.onChange((value) => this.accept(value));
        void api
          .snapshot()
          .then((result) => {
            if (result.ok) this.accept(result.value);
          })
          .catch(() => undefined);
      }
    }
    return () => {
      this.#listeners.delete(listener);
      if (this.#listeners.size === 0) {
        this.#stop?.();
        this.#stop = undefined;
      }
    };
  };

  accept(value: unknown): void {
    if (!isProjectionCompatibilitySnapshot(value)) return;
    const current = this.#state.snapshot;
    if (current && BigInt(value.revision) <= BigInt(current.revision)) return;
    this.#state = { ...this.#state, snapshot: structuredClone(value) };
    this.#notify();
  }

  ownRoute(route: ProjectionUnsubscribeRequest): () => void {
    const key = projectionCompatibilityRouteKey(route);
    this.#routes.set(key, (this.#routes.get(key) ?? 0) + 1);
    this.#routesChanged();
    let active = true;
    return () => {
      if (!active) return;
      active = false;
      const left = (this.#routes.get(key) ?? 1) - 1;
      if (left === 0) this.#routes.delete(key);
      else this.#routes.set(key, left);
      this.#routesChanged();
    };
  }

  reset(): void {
    this.#stop?.();
    this.#stop = undefined;
    this.#routes.clear();
    this.#state = { activeRoutes: new Set() };
    this.#notify();
  }
  #routesChanged(): void {
    this.#state = { ...this.#state, activeRoutes: new Set(this.#routes.keys()) };
    this.#notify();
  }
  #notify(): void {
    for (const listener of this.#listeners) listener();
  }
}

export const projectionCompatibilityStore = new ProjectionCompatibilityStore(
  () => window.aura?.v1?.projectionCompatibility,
);
export const ownProjectionCompatibilityRoute = (route: ProjectionUnsubscribeRequest) =>
  projectionCompatibilityStore.ownRoute(route);
export const useProjectionCompatibility = () =>
  useSyncExternalStore(
    projectionCompatibilityStore.subscribe,
    projectionCompatibilityStore.getSnapshot,
    projectionCompatibilityStore.getSnapshot,
  );
