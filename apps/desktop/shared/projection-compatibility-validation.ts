import type {
  ProjectionCompatibilitySnapshotV1,
  ProjectionCompatibilityKey,
  ProjectionKey,
} from '@aura/contracts/generated';
import { isAuxiliaryProjectionKey } from './auxiliary-projection-validation.js';
import { exactRecord, isAuraId, isRecord, isRevision } from './projection-validation/common.js';
import { isProjectionScope } from './projection-validation.js';

export const projectionCompatibilityKey = (key: ProjectionCompatibilityKey): string =>
  JSON.stringify([key.runtime_instance_id, projectionCompatibilityRouteKey(key)]);

export const projectionCompatibilityRouteKey = (key: {
  projection_kind: string;
  projection_scope: unknown;
}): string =>
  JSON.stringify([
    key.projection_kind,
    Object.entries(key.projection_scope as Record<string, unknown>).sort(([left], [right]) =>
      left < right ? -1 : left > right ? 1 : 0,
    ),
  ]);

export const isProjectionCompatibilityKey = (value: unknown): value is ProjectionCompatibilityKey =>
  isAuxiliaryProjectionKey(value) || isStandardProjectionKey(value);

const isStandardProjectionKey = (value: unknown): value is ProjectionKey =>
  exactRecord(value, ['runtime_instance_id', 'projection_kind', 'projection_scope']) &&
  isAuraId(value.runtime_instance_id, 'run') &&
  isProjectionScope(value.projection_kind, value.projection_scope);

export const projectionKeyFromValue = (value: unknown): ProjectionKey | undefined => {
  if (!isRecord(value)) return undefined;
  const key = {
    runtime_instance_id: value.runtime_instance_id,
    projection_kind: value.projection_kind,
    projection_scope: value.projection_scope,
  };
  return isStandardProjectionKey(key) ? structuredClone(key) : undefined;
};

export const isProjectionCompatibilitySnapshot = (
  value: unknown,
): value is ProjectionCompatibilitySnapshotV1 =>
  exactRecord(value, [
    'contract_version',
    'runtime_instance_id',
    'revision',
    'paused_projections',
  ]) &&
  value.contract_version === 1 &&
  isAuraId(value.runtime_instance_id, 'run') &&
  isRevision(value.revision) &&
  Array.isArray(value.paused_projections) &&
  value.paused_projections.every(
    (key) =>
      isProjectionCompatibilityKey(key) && key.runtime_instance_id === value.runtime_instance_id,
  ) &&
  new Set(value.paused_projections.map(projectionCompatibilityKey)).size ===
    value.paused_projections.length;
