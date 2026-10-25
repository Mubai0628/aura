import type { AgentSessionProjection, ContextUsageSummaryView } from '@aura/contracts/generated';
import { renderHook } from '@testing-library/react';
import { expect, test, vi } from 'vitest';
import { selectInspectionStep, useLatestInspection } from './useLatestInspection';

const projection = (
  turns: {
    modelStepId: string;
    snapshotId?: string;
  }[][],
): AgentSessionProjection =>
  ({
    session_id: 'ses_fixture',
    runtime_instance_id: 'run_fixture',
    turns: turns.map((step) => ({
      assistant_messages: step.map(({ modelStepId, snapshotId }) => ({
        model_step_id: modelStepId,
        context_snapshot_id: snapshotId,
      })),
    })),
  }) as unknown as AgentSessionProjection;

test('the newest request with a snapshot is selected and marked current', () => {
  const selected = selectInspectionStep(
    projection([
      [{ modelStepId: 'stp_1', snapshotId: 'mex_1' }],
      [{ modelStepId: 'stp_2', snapshotId: 'mex_2' }],
    ]),
  );
  expect(selected).toEqual({ modelStepId: 'stp_2', current: true });
});

test('a step without a snapshot keeps the most recent real request of the same session', () => {
  const selected = selectInspectionStep(
    projection([
      [{ modelStepId: 'stp_1', snapshotId: 'mex_1' }],
      [{ modelStepId: 'stp_2', snapshotId: 'mex_2' }],
      [{ modelStepId: 'stp_3' }],
    ]),
  );
  expect(selected).toEqual({ modelStepId: 'stp_2', current: false });
});

test('a failed newest turn still falls back to an earlier durable request', () => {
  const selected = selectInspectionStep(
    projection([[{ modelStepId: 'stp_1', snapshotId: 'mex_1' }], [{ modelStepId: 'stp_2' }]]),
  );
  expect(selected).toEqual({ modelStepId: 'stp_1', current: false });
});

test('a session without any request selects nothing instead of inventing usage', () => {
  expect(selectInspectionStep(projection([[{ modelStepId: 'stp_1' }]]))).toBeUndefined();
  expect(selectInspectionStep(undefined)).toBeUndefined();
});

test('the ring reads the frozen meter on the projection and does not load the inspector', () => {
  const readAgentInspector = vi.fn();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentInspector } },
  });
  const usage = {
    estimated: false,
    runtime_effective_window_tokens: 1000,
    used_input_tokens: 40,
    segments: [],
  } as ContextUsageSummaryView;
  const session = {
    session_id: 'ses_fixture',
    runtime_instance_id: 'run_fixture',
    turns: [
      {
        assistant_messages: [
          {
            model_step_id: 'stp_1',
            context_snapshot_id: 'mex_1',
            context_usage: usage,
            context_compaction_running: true,
          },
        ],
      },
    ],
  } as unknown as AgentSessionProjection;
  const { result } = renderHook(() => useLatestInspection(session));
  expect(result.current.usage?.used_input_tokens).toBe(40);
  expect(result.current.compactionRunning).toBe(true);
  expect(result.current.source).toEqual({ kind: 'current' });
  expect(readAgentInspector).not.toHaveBeenCalled();
});

test('a step with a snapshot but no frozen meter is a miss, not an empty session', () => {
  const session = {
    session_id: 'ses_fixture',
    runtime_instance_id: 'run_fixture',
    turns: [
      {
        assistant_messages: [
          { model_step_id: 'stp_1', context_snapshot_id: 'mex_1' },
          { model_step_id: 'stp_2' },
        ],
      },
    ],
  } as unknown as AgentSessionProjection;
  const { result } = renderHook(() => useLatestInspection(session));
  expect(result.current.source).toEqual({ kind: 'failed' });
  expect(result.current.usage).toBeUndefined();
  expect(result.current.modelStepId).toBe('stp_1');
});
