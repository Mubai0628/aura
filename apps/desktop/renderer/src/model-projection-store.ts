import type { ModelProjectionChange, ModelProjectionRouteRequest } from '../../shared/aura-api';
import { modelProjectionRouteKey } from '../../shared/projection-validation';
import { useCallback, useSyncExternalStore } from 'react';
import { ownProjectionCompatibilityRoute } from './projection-compatibility-store';

type ProjectionListener = () => void;

interface ProjectionRouteEntry {
  listeners: Set<ProjectionListener>;
  route: ModelProjectionRouteRequest;
  stopCompatibility: () => void;
}

const routes = new Map<string, ProjectionRouteEntry>();
const cachedChanges = new Map<string, ModelProjectionChange>();
const cachedErrors = new Map<string, string>();
let stopListening: (() => void) | undefined;

const isNewerProjection = (
  current: ModelProjectionChange | undefined,
  next: ModelProjectionChange,
): boolean => {
  if (current === undefined || current.runtime_instance_id !== next.runtime_instance_id)
    return true;
  return BigInt(next.revision) > BigInt(current.revision);
};

const ensureProjectionListener = () => {
  if (stopListening !== undefined) return;
  stopListening = window.aura.v1.onModelProjection((change) => {
    const route = {
      projection_kind: change.projection_kind,
      projection_scope: change.projection_scope,
    } as ModelProjectionRouteRequest;
    const key = modelProjectionRouteKey(route);
    if (!isNewerProjection(cachedChanges.get(key), change)) return;
    cachedChanges.set(key, change);
    cachedErrors.delete(key);
    const entry = routes.get(key);
    if (entry === undefined) return;
    for (const listener of entry.listeners) listener();
  });
};

const stopProjectionListenerWhenIdle = () => {
  if (routes.size !== 0 || stopListening === undefined) return;
  stopListening();
  stopListening = undefined;
};

/**
 * Shares one Core route among all Renderer consumers of the same Kind + Scope.
 * Cached projections deliberately survive the final unsubscribe, allowing a
 * returning page to render its last coherent revision while Core resubscribes.
 */
export const subscribeScopedModelProjection = (
  route: ModelProjectionRouteRequest,
  listener: ProjectionListener,
): (() => void) => {
  const key = modelProjectionRouteKey(route);
  let entry = routes.get(key);
  if (entry === undefined) {
    entry = {
      listeners: new Set(),
      route,
      stopCompatibility: ownProjectionCompatibilityRoute(route),
    };
    routes.set(key, entry);
    cachedErrors.delete(key);
    entry.listeners.add(listener);
    ensureProjectionListener();
    const currentEntry = entry;
    const fail = (message: string) => {
      if (routes.get(key) !== currentEntry) return;
      cachedErrors.set(key, message);
      for (const notify of currentEntry.listeners) notify();
    };
    void window.aura.v1
      .subscribeModelProjection(route)
      .then((result) => {
        if (!result.ok) fail(result.error.message);
      })
      .catch(() => fail('模型配置读取失败，请检查连接后重新打开会话。'));
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
    void window.aura.v1.unsubscribeModelProjection(current.route).then((result) => {
      if (!result.ok) {
        console.error('Scoped model projection unsubscription failed', result.error.code);
      }
    });
    stopProjectionListenerWhenIdle();
  };
};

type ModelProjectionKind = ModelProjectionChange['projection_kind'];
type ModelProjectionChangeFor<Kind extends ModelProjectionKind> = Extract<
  ModelProjectionChange,
  { projection_kind: Kind }
>;

export const useScopedModelProjection = <Kind extends ModelProjectionKind>(
  route: Extract<ModelProjectionRouteRequest, { projection_kind: Kind }>,
  disabled = false,
): ModelProjectionChangeFor<Kind> | undefined => {
  const routeKey = modelProjectionRouteKey(route);
  const subscribe = useCallback(
    (listener: ProjectionListener) =>
      disabled ? () => undefined : subscribeScopedModelProjection(route, listener),
    [disabled, route],
  );
  const getSnapshot = useCallback(
    () =>
      disabled
        ? undefined
        : (cachedChanges.get(routeKey) as ModelProjectionChangeFor<Kind> | undefined),
    [disabled, routeKey],
  );
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
};

export const resetScopedModelProjectionStoreForTests = () => {
  for (const entry of routes.values()) entry.stopCompatibility();
  routes.clear();
  cachedChanges.clear();
  cachedErrors.clear();
  stopListening?.();
  stopListening = undefined;
};

export function useModelProjectionError(
  route: ModelProjectionRouteRequest,
  disabled = false,
): string | undefined {
  const key = modelProjectionRouteKey(route);
  const subscribe = useCallback(
    (listener: ProjectionListener) =>
      disabled ? () => undefined : subscribeScopedModelProjection(route, listener),
    [route, disabled],
  );
  const snapshot = useCallback(
    () => (disabled ? undefined : cachedErrors.get(key)),
    [key, disabled],
  );
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}

/** A newly materialized conversation must read the authoritative binding CAS. */
export function readSessionModelSelector(
  sessionId: string,
): Promise<ModelProjectionChangeFor<'session_model_selector'>> {
  const route = {
    projection_kind: 'session_model_selector',
    projection_scope: { type: 'session', session_id: sessionId },
  } as const;
  const key = modelProjectionRouteKey(route);
  return new Promise((resolve, reject) => {
    let stop = () => undefined as void;
    const timer = window.setTimeout(() => {
      stop();
      reject(new Error('模型配置读取超时，请检查连接后重试。'));
    }, 15_000);
    const receive = () => {
      const error = cachedErrors.get(key);
      if (error) {
        window.clearTimeout(timer);
        stop();
        reject(new Error(error));
        return;
      }
      const value = cachedChanges.get(key);
      if (value?.projection_kind !== 'session_model_selector') return;
      window.clearTimeout(timer);
      stop();
      resolve(value);
    };
    stop = subscribeScopedModelProjection(route, receive);
    receive();
  });
}
