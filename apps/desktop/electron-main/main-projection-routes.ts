import { CORE_NOT_READY_MESSAGE } from './core-unavailable.js';
import {
  modelProjectionRouteKey,
  toolProjectionRouteKey,
  type ModelProjectionRouteRequest,
  type ToolProjectionRouteRequest,
} from '../shared/projection-validation.js';
import { CoreSupervisor } from './core-supervisor.js';

/**
 * Which window owns which model and tool projection subscription.
 * Core is asked to subscribe once per route; the last owner releasing it unsubscribes.
 */
export interface ProjectionRouteState {
  /** Main's current Core supervisor, read when a route is subscribed or released. */
  supervisor: () => CoreSupervisor | undefined;
  appendMainLog: (event: string, fields: Record<string, unknown>) => Promise<void>;
  modelProjectionRoutesByWebContents: Map<number, Set<string>>;
  modelProjectionRoutes: Map<string, { owners: Set<number>; route: ModelProjectionRouteRequest }>;
  toolProjectionRoutesByWebContents: Map<number, Set<string>>;
  toolProjectionRoutes: Map<string, { owners: Set<number>; route: ToolProjectionRouteRequest }>;
}

export function createProjectionRoutes(deps: ProjectionRouteState) {
  const {
    appendMainLog,
    modelProjectionRoutesByWebContents,
    modelProjectionRoutes,
    toolProjectionRoutesByWebContents,
    toolProjectionRoutes,
  } = deps;
  const subscribeModelProjectionRoute = async (
    webContentsId: number,
    route: ModelProjectionRouteRequest,
  ): Promise<void> => {
    const supervisor = deps.supervisor();
    const key = modelProjectionRouteKey(route);
    const owned = modelProjectionRoutesByWebContents.get(webContentsId) ?? new Set<string>();
    if (owned.has(key)) return;
    const existing = modelProjectionRoutes.get(key);
    if (existing !== undefined) {
      existing.owners.add(webContentsId);
      owned.add(key);
      modelProjectionRoutesByWebContents.set(webContentsId, owned);
      return;
    }

    if (supervisor === undefined) throw new Error(CORE_NOT_READY_MESSAGE);
    // Ownership is registered before Core acknowledges the subscription: Core
    // publishes the current snapshot immediately after that acknowledgement, and
    // `publishModelProjection` drops any change whose scoped route Main does not
    // own yet. Awaiting first lost that first snapshot intermittently, leaving a
    // conversation without its model selector until some later change.
    const registered = {
      owners: new Set([webContentsId]),
      route: structuredClone(route),
    };
    modelProjectionRoutes.set(key, registered);
    owned.add(key);
    modelProjectionRoutesByWebContents.set(webContentsId, owned);
    try {
      await supervisor.subscribeModelProjection(route);
    } catch (error) {
      modelProjectionRoutes.delete(key);
      owned.delete(key);
      if (owned.size === 0) modelProjectionRoutesByWebContents.delete(webContentsId);
      throw error;
    }
  };

  const unsubscribeModelProjectionRoute = async (
    webContentsId: number,
    route: ModelProjectionRouteRequest,
  ): Promise<void> => {
    const supervisor = deps.supervisor();
    const key = modelProjectionRouteKey(route);
    const owned = modelProjectionRoutesByWebContents.get(webContentsId);
    if (owned === undefined || !owned.delete(key)) return;
    if (owned.size === 0) modelProjectionRoutesByWebContents.delete(webContentsId);
    const existing = modelProjectionRoutes.get(key);
    if (existing === undefined) return;
    existing.owners.delete(webContentsId);
    if (existing.owners.size !== 0) return;
    modelProjectionRoutes.delete(key);
    await supervisor?.unsubscribeModelProjection(existing.route);
  };

  const releaseModelProjectionRoutes = async (webContentsId: number): Promise<void> => {
    const keys = [...(modelProjectionRoutesByWebContents.get(webContentsId) ?? [])];
    for (const key of keys) {
      const route = modelProjectionRoutes.get(key)?.route;
      if (route !== undefined) {
        try {
          await unsubscribeModelProjectionRoute(webContentsId, route);
        } catch (error) {
          await appendMainLog('model_projection_unsubscribe_failed', {
            message: error instanceof Error ? error.message : String(error),
            route: key,
          }).catch(() => undefined);
        }
      }
    }
    modelProjectionRoutesByWebContents.delete(webContentsId);
  };

  const subscribeToolProjectionRoute = async (
    webContentsId: number,
    route: ToolProjectionRouteRequest,
  ): Promise<void> => {
    const supervisor = deps.supervisor();
    const key = toolProjectionRouteKey(route);
    const owned = toolProjectionRoutesByWebContents.get(webContentsId) ?? new Set<string>();
    if (owned.has(key)) return;
    const existing = toolProjectionRoutes.get(key);
    if (existing !== undefined) {
      existing.owners.add(webContentsId);
      owned.add(key);
      toolProjectionRoutesByWebContents.set(webContentsId, owned);
      return;
    }

    if (supervisor === undefined) throw new Error(CORE_NOT_READY_MESSAGE);
    const registered = {
      owners: new Set([webContentsId]),
      route: structuredClone(route),
    };
    toolProjectionRoutes.set(key, registered);
    owned.add(key);
    toolProjectionRoutesByWebContents.set(webContentsId, owned);
    try {
      await supervisor.subscribeToolProjection(route);
    } catch (error) {
      toolProjectionRoutes.delete(key);
      owned.delete(key);
      if (owned.size === 0) toolProjectionRoutesByWebContents.delete(webContentsId);
      throw error;
    }
  };

  const unsubscribeToolProjectionRoute = async (
    webContentsId: number,
    route: ToolProjectionRouteRequest,
  ): Promise<void> => {
    const supervisor = deps.supervisor();
    const key = toolProjectionRouteKey(route);
    const owned = toolProjectionRoutesByWebContents.get(webContentsId);
    if (owned === undefined || !owned.delete(key)) return;
    if (owned.size === 0) toolProjectionRoutesByWebContents.delete(webContentsId);
    const existing = toolProjectionRoutes.get(key);
    if (existing === undefined) return;
    existing.owners.delete(webContentsId);
    if (existing.owners.size !== 0) return;
    toolProjectionRoutes.delete(key);
    await supervisor?.unsubscribeToolProjection(existing.route);
  };

  const releaseToolProjectionRoutes = async (webContentsId: number): Promise<void> => {
    const keys = [...(toolProjectionRoutesByWebContents.get(webContentsId) ?? [])];
    for (const key of keys) {
      const route = toolProjectionRoutes.get(key)?.route;
      if (route !== undefined) {
        try {
          await unsubscribeToolProjectionRoute(webContentsId, route);
        } catch (error) {
          await appendMainLog('tool_projection_unsubscribe_failed', {
            message: error instanceof Error ? error.message : String(error),
            route: key,
          }).catch(() => undefined);
        }
      }
    }
    toolProjectionRoutesByWebContents.delete(webContentsId);
  };
  return {
    subscribeModelProjectionRoute,
    unsubscribeModelProjectionRoute,
    releaseModelProjectionRoutes,
    subscribeToolProjectionRoute,
    unsubscribeToolProjectionRoute,
    releaseToolProjectionRoutes,
  };
}
