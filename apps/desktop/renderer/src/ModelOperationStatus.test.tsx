import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ModelAvailabilityTestView } from '@aura/contracts/generated';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

const scopedProjection = vi.hoisted(() => ({ current: undefined as unknown }));

vi.mock('./model-projection-store', () => ({
  useScopedModelProjection: () => scopedProjection.current,
}));

import { ModelAvailabilityStatus } from './ModelOperationStatus';

const TEST_ID = 'mav_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const ENDPOINT_ID = 'end_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const PROFILE_ID = 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAV';

const testView = (
  state: ModelAvailabilityTestView['state'],
  revision: string,
): ModelAvailabilityTestView => ({
  cancellation_reason: state === 'cancelled' ? 'user_requested' : null,
  completed_at: state === 'cancelled' ? '2026-09-01T02:00:01Z' : null,
  endpoint_id: ENDPOINT_ID,
  endpoint_revision_at_start: '4',
  endpoint_security_fingerprint: {
    algorithm: 'sha256',
    hex: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
  },
  id: TEST_ID,
  model_profile_id: PROFILE_ID,
  model_profile_revision_at_start: '3',
  models_revision_at_start: '8',
  result: null,
  revision,
  stage: state === 'queued' ? null : 'network',
  started_at: '2026-09-01T02:00:00Z',
  state,
});

const projection = (test: ModelAvailabilityTestView) => ({
  payload: {
    model_availability_test_id: TEST_ID,
    test,
  },
  projection_kind: 'model_availability_test' as const,
  projection_scope: {
    model_availability_test_id: TEST_ID,
    type: 'model_availability_test' as const,
  },
  revision: test.revision,
  runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
});

beforeEach(() => {
  scopedProjection.current = undefined;
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        cancelModelAvailabilityTest: vi.fn(),
      },
    },
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe('ModelAvailabilityStatus', () => {
  test('retries one revision conflict from the direct scoped projection and reports its terminal view', async () => {
    const user = userEvent.setup();
    const cancel = vi.mocked(window.aura.v1.cancelModelAvailabilityTest);
    cancel
      .mockResolvedValueOnce({
        error: {
          category: 'conflict',
          code: 'operation.revision_conflict',
          correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
          details: { actual_revision: '2', expected_revision: '1' },
          message: 'The operation changed.',
          retryable: true,
          terminal: 'failed',
        },
        ok: false,
      })
      .mockResolvedValueOnce({
        ok: true,
        value: {
          model_availability_test_id: TEST_ID,
          operational_revision: '3',
          state: 'cancelled',
        },
      });
    const onNotify = vi.fn();
    const onTerminal = vi.fn();
    const seed = testView('queued', '1');
    const rendered = render(
      <ModelAvailabilityStatus onNotify={onNotify} onTerminal={onTerminal} test={seed} />,
    );

    expect(screen.getByText(/请求可能已经到达 Provider/u)).toBeTruthy();
    await user.click(screen.getByRole('button', { name: '取消' }));
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(cancel).toHaveBeenLastCalledWith({
      expected_operational_revision: '1',
      model_availability_test_id: TEST_ID,
    });

    const running = testView('running', '2');
    scopedProjection.current = projection(running);
    rendered.rerender(
      <ModelAvailabilityStatus onNotify={onNotify} onTerminal={onTerminal} test={seed} />,
    );
    await waitFor(() => expect(cancel).toHaveBeenCalledTimes(2));
    expect(cancel).toHaveBeenLastCalledWith({
      expected_operational_revision: '2',
      model_availability_test_id: TEST_ID,
    });

    const cancelled = testView('cancelled', '3');
    scopedProjection.current = projection(cancelled);
    rendered.rerender(
      <ModelAvailabilityStatus onNotify={onNotify} onTerminal={onTerminal} test={seed} />,
    );
    await waitFor(() => expect(onTerminal).toHaveBeenCalledWith(cancelled));
    expect(screen.queryByRole('button', { name: '取消' })).toBeNull();
    expect(screen.getByText(/可用性：已取消/u)).toBeTruthy();
    expect(onNotify).not.toHaveBeenCalled();
  });
});
