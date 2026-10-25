import type {
  CoreStartupProgress,
  RuntimeHandshake,
  RuntimeHandshakeTranscript,
  RuntimeHeartbeat,
} from '@aura/contracts/generated';
import { createHash } from 'node:crypto';
import type { ReleaseContext } from '../release.js';

/** Handshake transcripts and identities, startup progress and heartbeats. */
export const delay = async (milliseconds: number): Promise<void> =>
  new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));

export const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

export const isLowerHex = (value: string, length: number): boolean =>
  value.length === length && /^[0-9a-f]+$/u.test(value);

export const canonicalizeJson = (value: unknown): unknown => {
  if (Array.isArray(value)) return value.map(canonicalizeJson);
  if (!isRecord(value)) return value;
  return Object.fromEntries(
    Object.keys(value)
      .sort()
      .map((key) => [key, canonicalizeJson(value[key])]),
  );
};

export const hashRuntimeHandshakeTranscript = (transcript: RuntimeHandshakeTranscript): string =>
  createHash('sha256')
    .update(JSON.stringify(canonicalizeJson(transcript)), 'utf8')
    .digest('hex');

export const sameRuntimeHandshake = (left: RuntimeHandshake, right: RuntimeHandshake): boolean =>
  JSON.stringify(canonicalizeJson(left)) === JSON.stringify(canonicalizeJson(right));

export const frozenCoreHandshake = (
  handshake: RuntimeHandshake,
  release: ReleaseContext,
): RuntimeHandshake => ({
  ...structuredClone(handshake),
  release_descriptor: structuredClone(release.releaseDescriptor),
});

export const isNativeProcessIdentityFailure = (error: unknown): boolean =>
  isRecord(error) && error.code === 'ERR_AURA_PIPE_PROCESS_IDENTITY';

export const parseStartupProgress = (value: unknown): CoreStartupProgress => {
  if (
    !isRecord(value) ||
    typeof value.runtime_instance_id !== 'string' ||
    typeof value.sequence !== 'number' ||
    typeof value.phase !== 'string' ||
    typeof value.message !== 'string' ||
    typeof value.occurred_at !== 'string'
  ) {
    throw new Error('invalid runtime.startup_progress payload');
  }
  return value as unknown as CoreStartupProgress;
};

export const parseHeartbeat = (value: unknown): RuntimeHeartbeat => {
  if (
    !isRecord(value) ||
    typeof value.runtime_instance_id !== 'string' ||
    typeof value.sequence !== 'number' ||
    typeof value.sent_at !== 'string'
  ) {
    throw new Error('invalid runtime.heartbeat payload');
  }
  return value as unknown as RuntimeHeartbeat;
};
