import type { WebContents } from 'electron';
import type { ModelProjectionChange, ToolProjectionChange } from '../shared/aura-api.js';
import {
  modelProjectionRouteKey,
  toolProjectionRouteKey,
  type ModelProjectionRouteRequest,
  type ToolProjectionRouteRequest,
} from '../shared/projection-validation.js';
import { publishToRenderer } from './publish-to-renderer.js';
export const MODEL_PROJECTION_EVENT_CHANNEL = 'aura:v1:model-projection';
export const TOOL_PROJECTION_EVENT_CHANNEL = 'aura:v1:tool-projection';
type OwnedRoutes = ReadonlyMap<number, ReadonlySet<string>>;

export function publishScopedModelProjection(
  contents: WebContents | undefined,
  change: ModelProjectionChange,
  routes: OwnedRoutes,
): void {
  if (!contents || contents.isDestroyed()) return;
  if (
    change.projection_scope.type !== 'global' &&
    !routes.get(contents.id)?.has(
      modelProjectionRouteKey({
        projection_kind: change.projection_kind,
        projection_scope: change.projection_scope,
      } as ModelProjectionRouteRequest),
    )
  )
    return;
  publishToRenderer(contents, MODEL_PROJECTION_EVENT_CHANNEL, change);
}

export function publishScopedToolProjection(
  contents: WebContents | undefined,
  change: ToolProjectionChange,
  routes: OwnedRoutes,
): void {
  if (!contents || contents.isDestroyed()) return;
  if (
    !routes.get(contents.id)?.has(
      toolProjectionRouteKey({
        projection_kind: change.projection_kind,
        projection_scope: change.projection_scope,
      } as ToolProjectionRouteRequest),
    )
  )
    return;
  publishToRenderer(contents, TOOL_PROJECTION_EVENT_CHANNEL, change);
}
