import type { CoreHealthProjectionChange } from './core-supervisor/config.js';

/** Where a window's read of Core's health comes from. */
interface HealthSource {
  refreshSnapshot(): Promise<CoreHealthProjectionChange>;
  readonly snapshot: CoreHealthProjectionChange;
}

/**
 * Core's health for a window: Core's own snapshot, or Main's view of it when Core is too busy to
 * answer the read, which Core's changes keep current; without a supervisor, `fallback`.
 */
export const healthForWindow = async (
  supervisor: HealthSource | undefined,
  fallback: CoreHealthProjectionChange,
): Promise<CoreHealthProjectionChange> =>
  supervisor === undefined
    ? fallback
    : supervisor.refreshSnapshot().catch(() => supervisor.snapshot);
