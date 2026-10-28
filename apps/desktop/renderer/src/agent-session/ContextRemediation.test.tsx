import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import {
  OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  REQUEST_TIMEOUT_CODE,
} from '../../../shared/ipc-request-failure';
import { ContextRemediation } from './ContextRemediation';

const read = vi.fn();
const recheck = vi.fn();
const submit = vi.fn();
const value = (session_id: string, agent_run_id: string, state = 'awaiting') => ({
  session_id,
  agent_run_id,
  remediation: {
    model_step_id: `step:${agent_run_id}`,
    revision: 3,
    state,
    budget: { estimated_input_tokens: 18000, request_input_budget_tokens: 16000 },
    plan: {
      overflow: { non_compressible_tokens: 15000, user_reducible_tokens: 3000 },
      actions: ['reduce_user_managed_context'],
    },
  },
});
beforeEach(() => {
  read
    .mockReset()
    .mockImplementation(({ session_id, agent_run_id }) =>
      Promise.resolve({ ok: true, value: value(session_id, agent_run_id) }),
    );
  recheck.mockReset();
  submit.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        readAgentContextRemediation: read,
        recheckAgentContext: recheck,
        submitAgentMessage: submit,
      },
    },
  });
});
afterEach(cleanup);
const props = {
  sessionId: 's',
  runId: 'r',
  onChanged: vi.fn(),
  onOpenModelSettings: vi.fn(),
  onStop: vi.fn(),
};

test('recheck binds the actual preflight revision and creates no user message', async () => {
  recheck.mockResolvedValue({ ok: true, value: value('s', 'r', 'rechecking') });
  render(<ContextRemediation {...props} />);
  await screen.findByText(/预计输入 18,000/);
  fireEvent.click(screen.getByRole('button', { name: '重新检查' }));
  await waitFor(() =>
    expect(recheck).toHaveBeenCalledWith({
      session_id: 's',
      agent_run_id: 'r',
      source_model_step_id: 'step:r',
      expected_revision: 3,
    }),
  );
  expect(submit).not.toHaveBeenCalled();
  await waitFor(() =>
    expect((screen.getByRole('button', { name: '重新检查' }) as HTMLButtonElement).disabled).toBe(
      true,
    ),
  );
});

test('a request the provider refused as too long says so, though it was counted within the budget', async () => {
  read.mockImplementation(({ session_id, agent_run_id }) => {
    const answer = value(session_id, agent_run_id);
    answer.remediation.budget.estimated_input_tokens = 9000;
    Object.assign(answer.remediation.plan.overflow, {
      error_code: 'context.provider_rebuild_no_progress',
    });
    return Promise.resolve({ ok: true, value: answer });
  });
  render(<ContextRemediation {...props} />);
  await screen.findByText('模型服务商拒绝了这次请求：超出它的上下文窗口，压缩后也没能缩小。');
  screen.getByText(/预计输入 9,000/);
});

test('a request counted past the budget does not claim a provider refusal', async () => {
  render(<ContextRemediation {...props} />);
  await screen.findByText(/预计输入 18,000/);
  expect(screen.queryByText(/模型服务商拒绝了这次请求/)).toBeNull();
});

test('uncertain result prevents another submission across a remount', async () => {
  recheck.mockRejectedValue(new Error('disconnected'));
  const first = render(<ContextRemediation {...props} runId="uncertain" />);
  await screen.findByText(/预计输入/);
  fireEvent.click(screen.getByRole('button', { name: '重新检查' }));
  await screen.findByRole('button', { name: '已核对状态' });
  first.unmount();
  render(<ContextRemediation {...props} runId="uncertain" />);
  await screen.findByText(/预计输入/);
  expect((screen.getByRole('button', { name: '重新检查' }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  expect(recheck).toHaveBeenCalledTimes(1);
});

test('a request-local timeout keeps the result pending confirmation', async () => {
  recheck.mockResolvedValue({
    error: {
      category: 'cancelled',
      code: REQUEST_TIMEOUT_CODE,
      correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      details: {
        command_invocation_id: 'civ_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        outcome_recovery: OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
      },
      message: 'Core request timed out: agent.context.recheck',
      retryable: false,
      terminal: 'cancelled',
    },
    ok: false,
  });
  render(<ContextRemediation {...props} runId="timed-out" />);
  await screen.findByText(/预计输入/);
  fireEvent.click(screen.getByRole('button', { name: '重新检查' }));
  // The recheck may already have run, so the button that would repeat it stays disabled until
  // the user confirms the result.
  await screen.findByRole('button', { name: '已核对状态' });
  expect((screen.getByRole('button', { name: '重新检查' }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  expect(recheck).toHaveBeenCalledTimes(1);
});

test('a late status from another session cannot replace the selected task', async () => {
  let resolveOld: (result: unknown) => void = () => undefined;
  read.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        resolveOld = resolve;
      }),
  );
  const rendered = render(<ContextRemediation {...props} key="s" />);
  rendered.rerender(<ContextRemediation {...props} sessionId="new" runId="new-run" key="new" />);
  await screen.findByText(/预计输入/);
  await act(async () => {
    resolveOld({ ok: true, value: value('s', 'r') });
  });
  recheck.mockResolvedValue({ ok: true, value: value('new', 'new-run', 'rechecking') });
  fireEvent.click(screen.getByRole('button', { name: '重新检查' }));
  await waitFor(() =>
    expect(recheck).toHaveBeenCalledWith(
      expect.objectContaining({ session_id: 'new', agent_run_id: 'new-run' }),
    ),
  );
});
