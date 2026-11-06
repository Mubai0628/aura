import { cleanup, render, screen } from '@testing-library/react';
import type { ModelAvailabilityTestResult } from '@aura/contracts/generated';
import { afterEach, expect, test } from 'vitest';
import { ModelAvailabilityCheckList } from '../ModelOperationStatus';

afterEach(cleanup);

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

// The checks share a model card's width three to a row; a sentence saying why one failed or did
// not run was squeezed into a third of it, a character or two to a line.
test('a check that did not pass takes the whole row, and one that passed keeps its column', () => {
  const result = {
    config: { status: 'passed' },
    network: { status: 'passed', latency_ms: 12 },
    tls: { status: 'not_run', reason_code: 'not_applicable_for_scheme' },
    http: { status: 'passed' },
    authentication: { status: 'not_run', reason_code: 'no_authentication_required' },
    model_generation: {
      status: 'failed',
      error_code: 'provider.account_region',
      provider_status: 403,
    },
  } as unknown as ModelAvailabilityTestResult;
  render(<ModelAvailabilityCheckList result={result} />);
  const row = (label: string) => screen.getByText(label, { selector: 'dt' }).parentElement;
  for (const label of ['TLS', '认证', '模型生成']) {
    expect(row(label)?.classList.contains('is-wide')).toBe(true);
  }
  for (const label of ['配置', '网络', 'HTTP']) {
    expect(row(label)?.classList.contains('is-wide')).toBe(false);
  }
});

// A setting of the user's account at the Provider refused the model; the key was accepted.
test('availability checks name the account setting a Provider refused the model by', () => {
  const sentences: [string, string][] = [
    [
      'provider.account_region',
      '服务商账号的区域设置不允许这个模型，请在服务商那边开放对应区域，或换一个模型',
    ],
    [
      'provider.account_data_policy',
      '服务商账号的隐私设置不允许这个模型的服务节点，请在服务商那边修改，或换一个模型',
    ],
    [
      'provider.client_not_allowed',
      '这个套餐只允许名单上的客户端，拒绝了 Aura，请试这家服务商其他协议的连接，或改用普通 API',
    ],
  ];
  for (const [code, sentence] of sentences) {
    const result = {
      config: { status: 'passed' },
      network: { status: 'passed' },
      tls: { status: 'passed' },
      http: { status: 'passed' },
      authentication: { status: 'passed' },
      model_generation: { status: 'failed', error_code: code, provider_status: 403 },
    } as unknown as ModelAvailabilityTestResult;
    const { unmount } = render(<ModelAvailabilityCheckList result={result} />);
    expect(screen.getByText(`失败 · ${sentence}（HTTP 403）`)).toBeTruthy();
    expect(screen.getByTitle(code)).toBeTruthy();
    unmount();
  }
});
