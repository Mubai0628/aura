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
    const existing = modelProjectionRoutes.get(key);
    if (existing !== undefined) {
      existing.owners.add(webContentsId);
      owned.add(key);
      modelProjectionRoutesByWebContents.set(webContentsId, owned);
      // Held already: for an earlier page of this window, whose request came in after that page
      // began to go away, or for another window. Core is asked again and answers with the route's
      // current state, which the page that asked has not had.
      await supervisor?.subscribeModelProjection(route);
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
      // Only what this subscription registered: the window may have let go of the route and
      // taken it again since.
      if (modelProjectionRoutes.get(key) === registered) modelProjectionRoutes.delete(key);
      owned.delete(key);
      if (owned.size === 0 && modelProjectionRoutesByWebContents.get(webContentsId) === owned)
        modelProjectionRoutesByWebContents.delete(webContentsId);
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

  /**
   * A window's page lets go of all its model routes at once, before Core has answered any
   * release: a page that loads in the same window then takes each route afresh, and Core answers
   * that subscription with the route's current state.
   */
  const releaseModelProjectionRoutes = async (webContentsId: number): Promise<void> => {
    const keys = modelProjectionRoutesByWebContents.get(webContentsId) ?? new Set<string>();
    modelProjectionRoutesByWebContents.delete(webContentsId);
    const unowned: { key: string; route: ModelProjectionRouteRequest }[] = [];
    for (const key of keys) {
      const existing = modelProjectionRoutes.get(key);
      if (existing === undefined || !existing.owners.delete(webContentsId)) continue;
      if (existing.owners.size !== 0) continue;
      modelProjectionRoutes.delete(key);
      unowned.push({ key, route: existing.route });
    }
    const supervisor = deps.supervisor();
    await Promise.all(
      unowned.map(async ({ key, route }) => {
        try {
          await supervisor?.unsubscribeModelProjection(route);
        } catch (error) {
          await appendMainLog('model_projection_unsubscribe_failed', {
            message: error instanceof Error ? error.message : String(error),
            route: key,
          }).catch(() => undefined);
        }
      }),
    );
  };

  const subscribeToolProjectionRoute = async (
    webContentsId: number,
    route: ToolProjectionRouteRequest,
  ): Promise<void> => {
    const supervisor = deps.supervisor();
    const key = toolProjectionRouteKey(route);
    const owned = toolProjectionRoutesByWebContents.get(webContentsId) ?? new Set<string>();
    const existing = toolProjectionRoutes.get(key);
    if (existing !== undefined) {
      existing.owners.add(webContentsId);
      owned.add(key);
      toolProjectionRoutesByWebContents.set(webContentsId, owned);
      // As a model route held already: asked again for its current state.
      await supervisor?.subscribeToolProjection(route);
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
      if (toolProjectionRoutes.get(key) === registered) toolProjectionRoutes.delete(key);
      owned.delete(key);
      if (owned.size === 0 && toolProjectionRoutesByWebContents.get(webContentsId) === owned)
        toolProjectionRoutesByWebContents.delete(webContentsId);
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

  /** As the model routes: all at once, before Core has answered any release. */
  const releaseToolProjectionRoutes = async (webContentsId: number): Promise<void> => {
    const keys = toolProjectionRoutesByWebContents.get(webContentsId) ?? new Set<string>();
    toolProjectionRoutesByWebContents.delete(webContentsId);
    const unowned: { key: string; route: ToolProjectionRouteRequest }[] = [];
    for (const key of keys) {
      const existing = toolProjectionRoutes.get(key);
      if (existing === undefined || !existing.owners.delete(webContentsId)) continue;
      if (existing.owners.size !== 0) continue;
      toolProjectionRoutes.delete(key);
      unowned.push({ key, route: existing.route });
    }
    const supervisor = deps.supervisor();
    await Promise.all(
      unowned.map(async ({ key, route }) => {
        try {
          await supervisor?.unsubscribeToolProjection(route);
        } catch (error) {
          await appendMainLog('tool_projection_unsubscribe_failed', {
            message: error instanceof Error ? error.message : String(error),
            route: key,
          }).catch(() => undefined);
        }
      }),
    );
  };

  /** A window's page went away: it loads again, or its renderer or its window is gone. */
  const releasePage = async (webContentsId: number): Promise<void> => {
    await Promise.all([
      releaseModelProjectionRoutes(webContentsId),
      releaseToolProjectionRoutes(webContentsId),
    ]);
  };
  return {
    subscribeModelProjectionRoute,
    unsubscribeModelProjectionRoute,
    releaseModelProjectionRoutes,
    subscribeToolProjectionRoute,
    unsubscribeToolProjectionRoute,
    releaseToolProjectionRoutes,
    releasePage,
  };
}
