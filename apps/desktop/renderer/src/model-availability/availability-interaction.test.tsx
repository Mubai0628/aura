import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type {
  ModelAvailabilityTestView,
  ModelEndpointConfig,
  ModelProfileConfig,
} from '@aura/contracts/generated';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ModelProfileCard } from '../ModelProfileCard';

const projections = vi.hoisted(() => ({
  test: undefined as ModelAvailabilityTestView | undefined,
}));
vi.mock('../model-projection-store', () => ({
  useScopedModelProjection: (route: { projection_kind: string }) =>
    route.projection_kind === 'model_profile'
      ? {
          payload: {
            latest_availability_test: { id: 'old-test', state: 'completed' },
            active_availability_test: null,
          },
        }
      : projections.test === undefined
        ? undefined
        : { payload: { test: projections.test } },
}));
const model: ModelProfileConfig = {
  id: 'model',
  endpoint_id: 'endpoint',
  display_name: 'Test Model',
  provider_model_name: 'test-model',
  origin: 'manual',
  created_at: '2026-09-22T00:00:00Z',
  updated_at: '2026-09-22T00:00:00Z',
  revision: '2',
  enabled: true,
  available_for_subagents: false,
  request_defaults: { max_output_tokens: 1024 },
  context: {
    runtime_effective_window_tokens: 8192,
    historical_snapshot_max_tokens: 1024,
    user_override_revision: 0,
  },
};
const endpoint: ModelEndpointConfig = {
  id: 'endpoint',
  provider_id: 'provider',
  display_name: 'Test Endpoint',
  slug: 'test',
  base_url: 'https://example.test',
  api_format: 'open_ai_responses',
  auth_scheme: 'none',
  revision: '3',
  created_at: model.created_at,
  updated_at: model.updated_at,
  discovery_mode: { type: 'automatic' },
  max_parallel_model_requests: 1,
  model_order: [],
  timeouts: { connect_ms: 1000, first_event_ms: 1000, stream_idle_ms: 1000, total_ms: 10000 },
};
const notify = vi.fn();
const card = () => (
  <ModelProfileCard
    model={model}
    endpoint={endpoint}
    busy={false}
    credentialState={undefined}
    isDefault={false}
    onApplyCatalogUpdate={vi.fn()}
    onDeleteRequest={vi.fn()}
    onEdit={vi.fn()}
    onNotify={notify}
    onSetDefault={vi.fn()}
    onToggleSubagents={vi.fn()}
    writeUnavailableReason={undefined}
  />
);
beforeEach(() => {
  projections.test = undefined;
  notify.mockClear();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { testModelAvailability: vi.fn() } },
  });
});
afterEach(cleanup);

test('opening a card never starts a test or replays persisted results', () => {
  render(card());
  expect(window.aura.v1.testModelAvailability).not.toHaveBeenCalled();
  expect(screen.queryByLabelText('模型可用性测试进度')).toBeNull();
  expect(notify).not.toHaveBeenCalled();
});

test('click shows immediate skeleton, only its returned test can notify, reopening clears presentation', async () => {
  const user = userEvent.setup();
  let accept!: (value: Awaited<ReturnType<typeof window.aura.v1.testModelAvailability>>) => void;
  vi.mocked(window.aura.v1.testModelAvailability).mockImplementation(
    () =>
      new Promise((resolve) => {
        accept = resolve;
      }),
  );
  const view = render(card());
  await user.click(screen.getByRole('button', { name: '测试可用性' }));
  expect(screen.getByLabelText('模型可用性测试进度').getAttribute('aria-busy')).toBe('true');
  await act(async () => {
    accept({
      ok: true,
      value: { model_availability_test_id: 'new-test', operational_revision: '1', state: 'queued' },
    });
  });
  expect(screen.getByLabelText('模型可用性测试进度').getAttribute('aria-busy')).toBe('true');
  projections.test = {
    id: 'new-test',
    model_profile_id: model.id,
    endpoint_id: endpoint.id,
    state: 'cancelled',
    revision: '3',
    started_at: model.created_at,
    endpoint_revision_at_start: '3',
    model_profile_revision_at_start: '2',
    models_revision_at_start: '1',
    endpoint_security_fingerprint: { algorithm: 'sha256', hex: 'a'.repeat(64) },
  };
  view.rerender(card());
  await waitFor(() => expect(notify).toHaveBeenCalledTimes(1));
  expect(screen.getByText('可用性：已取消')).toBeTruthy();
  view.unmount();
  render(card());
  expect(screen.queryByLabelText('模型可用性测试进度')).toBeNull();
  expect(notify).toHaveBeenCalledTimes(1);
  expect(window.aura.v1.testModelAvailability).toHaveBeenCalledTimes(1);
});

test('a rejected start removes the skeleton and permits a new explicit attempt', async () => {
  vi.mocked(window.aura.v1.testModelAvailability).mockResolvedValue({
    ok: false,
    error: {
      category: 'unavailable',
      code: 'test.unavailable',
      message: 'Unavailable',
      retryable: false,
      terminal: 'failed',
      correlation_id: 'cor',
      details: {},
    },
  });
  render(card());
  await userEvent.click(screen.getByRole('button', { name: '测试可用性' }));
  await waitFor(() => expect(notify).toHaveBeenCalledTimes(1));
  expect(screen.queryByLabelText('模型可用性测试进度')).toBeNull();
  expect(screen.getByRole<HTMLButtonElement>('button', { name: '测试可用性' }).disabled).toBe(
    false,
  );
});
