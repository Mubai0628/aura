import type { ToolProjectionChange, ToolProjectionRouteRequest } from '../../shared/aura-api';
import { toolProjectionRouteKey } from '../../shared/projection-validation';
import { useCallback, useSyncExternalStore } from 'react';
import { ownProjectionCompatibilityRoute } from './projection-compatibility-store';

type ProjectionListener = () => void;

interface ProjectionRouteEntry {
  listeners: Set<ProjectionListener>;
  route: ToolProjectionRouteRequest;
  stopCompatibility: () => void;
}

const routes = new Map<string, ProjectionRouteEntry>();
const cachedChanges = new Map<string, ToolProjectionChange>();
const refreshes = new Map<string, Promise<void>>();
let stopListening: (() => void) | undefined;

const publish = (key: string, change: ToolProjectionChange): void => {
  const current = cachedChanges.get(key);
  if (
    current !== undefined &&
    current.runtime_instance_id === change.runtime_instance_id &&
    BigInt(change.revision) <= BigInt(current.revision)
  ) {
    return;
  }
  cachedChanges.set(key, change);
  const entry = routes.get(key);
  if (entry === undefined) return;
  for (const listener of entry.listeners) listener();
};

const refreshAfterGap = (key: string, route: ToolProjectionRouteRequest): void => {
  if (refreshes.has(key)) return;
  const refresh = window.aura.v1
    .refreshToolProjection(route)
    .then((result) => {
      if (!result.ok) {
        console.error('Scoped Tool Projection refresh failed', result.error.code);
        return;
      }
      publish(key, result.value);
    })
    .finally(() => refreshes.delete(key));
  refreshes.set(key, refresh);
};

const ensureProjectionListener = (): void => {
  if (stopListening !== undefined) return;
  stopListening = window.aura.v1.onToolProjection((change) => {
    const route = {
      projection_kind: change.projection_kind,
      projection_scope: change.projection_scope,
    } as ToolProjectionRouteRequest;
    const key = toolProjectionRouteKey(route);
    const current = cachedChanges.get(key);
    if (
      current !== undefined &&
      current.runtime_instance_id === change.runtime_instance_id &&
      BigInt(change.revision) > BigInt(current.revision) + 1n
    ) {
      refreshAfterGap(key, route);
      return;
    }
    publish(key, change);
  });
};

const stopProjectionListenerWhenIdle = (): void => {
  if (routes.size !== 0 || stopListening === undefined) return;
  stopListening();
  stopListening = undefined;
};

/**
 * Shares one exact P07 Kind + Scope subscription among Renderer consumers.
 * The last committed revision survives unmount, while a revision gap triggers
 * a snapshot refresh for only the affected scope.
 */
export const subscribeScopedToolProjection = (
  route: ToolProjectionRouteRequest,
  listener: ProjectionListener,
): (() => void) => {
  const key = toolProjectionRouteKey(route);
  let entry = routes.get(key);
  if (entry === undefined) {
    entry = {
      listeners: new Set(),
      route,
      stopCompatibility: ownProjectionCompatibilityRoute(route),
    };
    routes.set(key, entry);
    ensureProjectionListener();
    void window.aura.v1.subscribeToolProjection(route).then((result) => {
      if (!result.ok) {
        console.error('Scoped Tool Projection subscription failed', result.error.code);
      }
    });
    // A subscription only receives changes published after it exists, so a
    // projection committed earlier — a Registry head built before this surface was
    // opened, for instance — would stay undefined forever. The first subscriber for
    // an exact Kind + Scope therefore also reads the committed snapshot once; the
    // subscription still owns nothing but reading.
    refreshAfterGap(key, route);
  }
  entry.listeners.add(listener);

  let active = true;
  return () => {
    if (!active) return;
    active = false;
    const current = routes.get(key);
    if (current === undefined) return;
    current.listeners.delete(listener);
    if (current.listeners.size !== 0) return;
    routes.delete(key);
    current.stopCompatibility();
    void window.aura.v1.unsubscribeToolProjection(current.route).then((result) => {
      if (!result.ok) {
        console.error('Scoped Tool Projection unsubscription failed', result.error.code);
      }
    });
    stopProjectionListenerWhenIdle();
  };
};

type ToolProjectionKind = ToolProjectionChange['projection_kind'];
type ToolProjectionChangeFor<Kind extends ToolProjectionKind> = Extract<
  ToolProjectionChange,
  { projection_kind: Kind }
>;

export const useScopedToolProjection = <Kind extends ToolProjectionKind>(
  route: Extract<ToolProjectionRouteRequest, { projection_kind: Kind }>,
  disabled = false,
): ToolProjectionChangeFor<Kind> | undefined => {
  const routeKey = toolProjectionRouteKey(route);
  const subscribe = useCallback(
    (listener: ProjectionListener) =>
      disabled ? () => undefined : subscribeScopedToolProjection(route, listener),
    [disabled, route],
  );
  const getSnapshot = useCallback(
    () =>
      disabled
        ? undefined
        : (cachedChanges.get(routeKey) as ToolProjectionChangeFor<Kind> | undefined),
    [disabled, routeKey],
  );
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
};

export const resetScopedToolProjectionStoreForTests = (): void => {
  for (const entry of routes.values()) entry.stopCompatibility();
  routes.clear();
  cachedChanges.clear();
  refreshes.clear();
  stopListening?.();
  stopListening = undefined;
};
