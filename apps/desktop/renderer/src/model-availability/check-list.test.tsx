import { render, screen } from '@testing-library/react';
import type { ModelAvailabilityTestResult } from '@aura/contracts/generated';
import { expect, test } from 'vitest';
import { ModelAvailabilityCheckList } from '../ModelOperationStatus';

// A failed or skipped check read "失败 · authentication.rejected" or
// "未执行 · blocked_by_previous_stage". The words now say what went wrong; the code stays in the
// tooltip for diagnosis.
test('availability checks say in words what failed and why a check did not run', () => {
  const result = {
    config: { status: 'passed' },
    network: { status: 'passed', latency_ms: 12 },
    tls: { status: 'not_run', reason_code: 'not_applicable_for_scheme' },
    http: { status: 'failed', error_code: 'network.timeout', provider_status: null },
    authentication: {
      status: 'failed',
      error_code: 'authentication.rejected',
      provider_status: 401,
    },
    model_generation: { status: 'not_run', reason_code: 'blocked_by_previous_stage' },
  } as unknown as ModelAvailabilityTestResult;
  render(<ModelAvailabilityCheckList result={result} />);
  expect(screen.getByText('失败 · API Key 被拒绝，请检查后重新保存（HTTP 401）')).toBeTruthy();
  expect(screen.getByText('失败 · 连接超时，请检查网络与代理设置')).toBeTruthy();
  expect(screen.getByText('未执行 · 前一项没有通过')).toBeTruthy();
  expect(screen.getByText('未执行 · 不适用于这种连接方式')).toBeTruthy();
  expect(screen.getByText('通过 · 12 ms')).toBeTruthy();
  expect(screen.queryByText(/authentication\.rejected|blocked_by_previous_stage/u)).toBeNull();
  expect(screen.getByTitle('authentication.rejected')).toBeTruthy();
});
