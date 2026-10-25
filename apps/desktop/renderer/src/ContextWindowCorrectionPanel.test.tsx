import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { AgentContextWindowCorrection } from '@aura/contracts/generated';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import {
  OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  REQUEST_TIMEOUT_CODE,
} from '../../shared/ipc-request-failure';
import { ContextWindowCorrectionPanel } from './ContextWindowCorrectionPanel';

const settings = vi.fn();
const save = vi.fn();
const resolve = vi.fn();
const record: AgentContextWindowCorrection = {
  revision: '7',
  created_at: '2026-09-22T00:00:00Z',
  updated_at: '2026-09-22T00:00:00Z',
  state: 'awaiting',
  correction: {
    agent_run_id: 'r',
    model_step_id: 'step',
    model_profile_id: 'profile',
    failed_execution_snapshot_id: 'failed',
    provider_error: {
      provider_message: 'Context exceeds limit [redacted]',
      issue_code: 'model.context_window_exceeded',
      message_was_redacted: true,
      message_was_truncated: false,
    },
    resolution: {
      type: 'user_correction_required',
      current_effective_window_tokens: 256000,
      local_estimated_input_tokens: 16000,
      failed_execution_snapshot_id: 'failed',
      provider_error_ref: 'error',
      model_profile: {
        id: 'profile',
        endpoint_id: 'endpoint',
        display_name: 'Fixture',
        provider_model_name: 'fixture',
        enabled: true,
        available_for_subagents: false,
        origin: 'manual',
        revision: '1',
        created_at: '2026-09-22T00:00:00Z',
        updated_at: '2026-09-22T00:00:00Z',
        context: {
          runtime_effective_window_tokens: 256000,
          historical_snapshot_max_tokens: 1024,
          user_override_revision: 0,
        },
        request_defaults: {},
      },
      input_contract: { unit: 'tokens', minimum_tokens: 1, maximum_tokens: null },
    },
  },
};
const props = {
  sessionId: 's',
  record,
  onChanged: vi.fn(),
  onOpenModelSettings: vi.fn(),
  onStop: vi.fn(),
};
beforeEach(() => {
  settings.mockReset().mockResolvedValue({ ok: true, value: { models_revision: 9 } });
  save.mockReset().mockResolvedValue({ ok: true, value: { revision: 10 } });
  resolve
    .mockReset()
    .mockResolvedValue({ ok: true, value: { session_id: 's', agent_run_id: 'r' } });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getModelSettings: settings,
        correctModelContextWindow: save,
        resolveAgentContextWindow: resolve,
      },
    },
  });
});
afterEach(cleanup);

test('manual correction uses P05 authority then explicitly rechecks the bound failed step', async () => {
  render(<ContextWindowCorrectionPanel {...props} />);
  const input = screen.getByRole('textbox', { name: '完整上下文窗口（Token）' });
  fireEvent.change(input, { target: { value: '128K' } });
  expect(
    (screen.getByRole('button', { name: '保存并重新检查' }) as HTMLButtonElement).disabled,
  ).toBe(true);
  fireEvent.change(input, { target: { value: '128000' } });
  fireEvent.click(screen.getByRole('button', { name: '保存并重新检查' }));
  await waitFor(() =>
    expect(resolve).toHaveBeenCalledWith({
      session_id: 's',
      agent_run_id: 'r',
      source_model_step_id: 'step',
      expected_revision: '7',
      action: 'recheck',
    }),
  );
  expect(save).toHaveBeenCalledWith({
    expected_revision: 9,
    model_profile_id: 'profile',
    window_tokens: 128000,
  });
  expect(save.mock.invocationCallOrder[0]).toBeLessThan(resolve.mock.invocationCallOrder[0]!);
  expect(screen.getByText('Context exceeds limit [redacted]')).toBeTruthy();
});

test('cancelling manual correction requests candidates without writing model configuration', async () => {
  render(<ContextWindowCorrectionPanel {...props} />);
  fireEvent.click(screen.getByRole('button', { name: '取消手工修正，尝试较小窗口' }));
  await waitFor(() =>
    expect(resolve).toHaveBeenCalledWith(
      expect.objectContaining({ action: 'try_smaller_windows', expected_revision: '7' }),
    ),
  );
  expect(settings).not.toHaveBeenCalled();
  expect(save).not.toHaveBeenCalled();
});

test('a request-local timeout on the window write stays pending confirmation', async () => {
  save.mockResolvedValue({
    error: {
      category: 'cancelled',
      code: REQUEST_TIMEOUT_CODE,
      correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      details: {
        command_invocation_id: 'civ_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        outcome_recovery: OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
      },
      message: 'Core request timed out: models.context_window.correct',
      retryable: false,
      terminal: 'cancelled',
    },
    ok: false,
  });
  const unique = { ...props, sessionId: 'timeout-window' };
  render(<ContextWindowCorrectionPanel {...unique} />);
  fireEvent.change(screen.getByRole('textbox'), { target: { value: '64000' } });
  fireEvent.click(screen.getByRole('button', { name: '保存并重新检查' }));
  // The write may already have been applied: the follow-up write is not sent, and the panel
  // asks the user to confirm the stored value first.
  await screen.findByRole('button', { name: '已核对状态' });
  expect(resolve).not.toHaveBeenCalled();
  expect(
    (screen.getByRole('button', { name: '保存并重新检查' }) as HTMLButtonElement).disabled,
  ).toBe(true);
});

test('unknown response across unmount retains the draft and blocks a duplicate write', async () => {
  let rejectSave: (error: Error) => void = () => undefined;
  save.mockImplementation(
    () =>
      new Promise((_, reject) => {
        rejectSave = reject;
      }),
  );
  const unique = { ...props, sessionId: 'unknown-window' };
  const first = render(<ContextWindowCorrectionPanel {...unique} />);
  fireEvent.change(screen.getByRole('textbox'), { target: { value: '64000' } });
  fireEvent.click(screen.getByRole('button', { name: '保存并重新检查' }));
  await waitFor(() => expect(save).toHaveBeenCalledOnce());
  first.unmount();
  await act(async () => {
    rejectSave(new Error('lost'));
  });
  render(<ContextWindowCorrectionPanel {...unique} />);
  expect((screen.getByRole('textbox') as HTMLInputElement).value).toBe('64000');
  expect(
    (screen.getByRole('button', { name: '保存并重新检查' }) as HTMLButtonElement).disabled,
  ).toBe(true);
  expect(resolve).not.toHaveBeenCalled();
});
