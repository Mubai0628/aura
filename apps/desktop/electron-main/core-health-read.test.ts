// @vitest-environment node
import { expect, test, vi } from 'vitest';
import type { CoreHealthProjectionChange } from './core-supervisor/config.js';
import { healthForWindow } from './core-health-read.js';

const health = (revision: string): CoreHealthProjectionChange =>
  ({
    payload: {
      read_only: false,
      restart_attempt: 0,
      safe_mode: false,
      state: 'ready',
      status_code: 'core.ready',
      status_message: 'Aura Core is ready.',
    },
    projection_kind: 'core_health',
    projection_scope: { type: 'global' },
    revision,
    runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  }) as CoreHealthProjectionChange;

// A Core too busy to answer the window's read is not a Core that failed to start: the window gets
// Main's own view of Core's health, which Core's changes keep current.
test("a read Core did not answer gives Main's own view of Core", async () => {
  const supervisor = {
    refreshSnapshot: vi.fn().mockRejectedValue(new Error('Core request timed out')),
    snapshot: health('7'),
  };
  await expect(healthForWindow(supervisor, health('1'))).resolves.toBe(supervisor.snapshot);
  supervisor.refreshSnapshot.mockResolvedValue(health('8'));
  await expect(healthForWindow(supervisor, health('1'))).resolves.toMatchObject({ revision: '8' });
  await expect(healthForWindow(undefined, health('1'))).resolves.toMatchObject({ revision: '1' });
});
