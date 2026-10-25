import type {
  MediaHostActionV1,
  MediaHostCancelV1,
  MediaHostCommandV1,
  MediaHostCompletedV1,
  MediaHostFrameV1,
  MediaHostOutcomeV1,
  MediaHostResultV1,
  MediaImageV1,
  ProjectionChange,
} from '@aura/contracts/generated';

// Core's serde serializer emits every nullable field; Main validates that exact wire shape.
export type MediaIdentity = MediaHostCancelV1;
export type MediaAction =
  | Extract<MediaHostActionV1, { kind: 'probe' }>
  | Required<Extract<MediaHostActionV1, { kind: 'frames' }>>;
export type MediaCommand = Omit<MediaHostCommandV1, 'action'> & { action: MediaAction };
export type MediaImage = MediaImageV1;
export type MediaProbe = Required<Extract<MediaHostCompletedV1, { kind: 'probe' }>>;
export type MediaFrames = Required<Extract<MediaHostCompletedV1, { kind: 'frames' }>> & {
  frames: Required<MediaHostFrameV1>[];
};
export type MediaDecoded = MediaProbe | MediaFrames;
export type MediaOutcome = MediaHostOutcomeV1;
export type MediaResult = MediaHostResultV1;
export interface MediaCore {
  requestWithBudget(method: string, params: unknown, timeoutMilliseconds: number): Promise<unknown>;
  subscribe?(
    listener: (change: Extract<ProjectionChange, { projection_kind: 'core_health' }>) => void,
  ): () => void;
}
export const READ_BYTES = 128 * 1024;
export const IMAGE_BYTES = 256 * 1024;
export const TOTAL_IMAGE_BYTES = 3 * 1024 * 1024;
export const RESULT_BYTES = 6 * 1024 * 1024;

export class MediaRefusal extends Error {}
export class MediaFailure extends Error {}
