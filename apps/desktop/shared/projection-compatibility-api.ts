import type { ProjectionCompatibilitySnapshotV1 } from '@aura/contracts/generated';
import type { ResultEnvelope } from './aura-api.js';

export const PROJECTION_COMPATIBILITY_SNAPSHOT_CHANNEL = 'aura:v1:projection-compatibility-get';
export const PROJECTION_COMPATIBILITY_EVENT_CHANNEL = 'aura:v1:projection-compatibility';
export interface ProjectionCompatibilityApiV1 {
  snapshot(): Promise<ResultEnvelope<ProjectionCompatibilitySnapshotV1>>;
  onChange(listener: (snapshot: ProjectionCompatibilitySnapshotV1) => void): () => void;
}
