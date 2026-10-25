import type {
  ModelAvailabilityCheckResult,
  ModelAvailabilityTestView,
  ModelAvailabilityTestResult,
  ModelDiscoveryStartAccepted,
  ProviderErrorSummaryView,
} from '@aura/contracts/generated';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { useScopedModelProjection } from './model-projection-store';
import type { NotificationInput } from './notification-center-store';
import { userMessage } from './agent-session/user-message';
import { MODEL_FAILURE_FALLBACK } from './provider-connection/notices';

interface OperationStatusProps {
  onNotify: (notification: NotificationInput) => void;
  onTerminal?: () => void;
}

export function ModelDiscoveryStatus({
  accepted,
  onNotify,
  onTerminal,
}: OperationStatusProps & { accepted: ModelDiscoveryStartAccepted }) {
  const route = useMemo(
    () =>
      ({
        projection_kind: 'model_discovery',
        projection_scope: {
          model_discovery_run_id: accepted.model_discovery_run_id,
          type: 'model_discovery',
        },
      }) as const,
    [accepted.model_discovery_run_id],
  );
  const change = useScopedModelProjection(route);
  const run = change?.payload.run;
  const [cancelling, setCancelling] = useState(false);
  const retryAfterRevision = useRef<string | undefined>(undefined);
  const terminalReported = useRef(false);
  const state = run?.state ?? accepted.state;
  const revision = run?.revision ?? accepted.operational_revision;

  useEffect(() => {
    if ((state === 'completed' || state === 'cancelled') && !terminalReported.current) {
      terminalReported.current = true;
      onTerminal?.();
    }
  }, [onTerminal, state]);

  const cancelAtRevision = useCallback(
    async (expectedRevision: string, retry: boolean) => {
      const result = await window.aura.v1.cancelModelDiscovery({
        expected_operational_revision: expectedRevision,
        model_discovery_run_id: accepted.model_discovery_run_id,
      });
      if (result.ok) {
        retryAfterRevision.current = undefined;
        return;
      }
      if (result.error.code === 'operation.revision_conflict' && !retry) {
        retryAfterRevision.current = expectedRevision;
        return;
      }
      setCancelling(false);
      retryAfterRevision.current = undefined;
      notifyOperationFailure(onNotify, '模型目录取消未完成', result.error);
    },
    [accepted.model_discovery_run_id, onNotify],
  );

  useEffect(() => {
    const previousRevision = retryAfterRevision.current;
    if (
      previousRevision === undefined ||
      state === 'completed' ||
      state === 'cancelled' ||
      revision === previousRevision
    ) {
      return;
    }
    retryAfterRevision.current = undefined;
    void cancelAtRevision(revision, true);
  }, [cancelAtRevision, revision, state]);

  const cancel = async () => {
    setCancelling(true);
    await cancelAtRevision(revision, false);
  };

  return (
    <section aria-label="模型目录请求进度" className="model-operation-status">
      <div>
        <strong>模型目录：{discoveryStateLabel(state, run?.outcome)}</strong>
        <span>
          {run?.stage ?? 'queued'} · Revision {revision}
        </span>
      </div>
      {run?.provider_error === null || run?.provider_error === undefined ? null : (
        <ProviderErrorSummary summary={run.provider_error} />
      )}
      {state === 'queued' || state === 'running' ? (
        <button disabled={cancelling} onClick={() => void cancel()} type="button">
          {cancelling ? '取消中…' : '取消'}
        </button>
      ) : null}
    </section>
  );
}

export function ModelAvailabilityStatus({
  test,
  onNotify,
  onTerminal,
}: Omit<OperationStatusProps, 'onTerminal'> & {
  onTerminal?: (test: ModelAvailabilityTestView) => void;
  test: ModelAvailabilityTestView;
}) {
  const route = useMemo(
    () =>
      ({
        projection_kind: 'model_availability_test',
        projection_scope: {
          model_availability_test_id: test.id,
          type: 'model_availability_test',
        },
      }) as const,
    [test.id],
  );
  const change = useScopedModelProjection(route);
  const currentTest = change?.payload.test ?? test;
  const [cancelling, setCancelling] = useState(false);
  const retryAfterRevision = useRef<string | undefined>(undefined);
  const terminalReported = useRef(false);
  const state = currentTest.state;
  const revision = currentTest.revision;

  useEffect(() => {
    if ((state === 'completed' || state === 'cancelled') && !terminalReported.current) {
      terminalReported.current = true;
      onTerminal?.(currentTest);
    }
  }, [currentTest, onTerminal, state]);

  const cancelAtRevision = useCallback(
    async (expectedRevision: string, retry: boolean) => {
      const result = await window.aura.v1.cancelModelAvailabilityTest({
        expected_operational_revision: expectedRevision,
        model_availability_test_id: test.id,
      });
      if (result.ok) {
        // The command only returns after the terminal scoped Projection has
        // been published. Keep the direct Projection as the UI fact source.
        retryAfterRevision.current = undefined;
        return;
      }
      if (result.error.code === 'operation.revision_conflict' && !retry) {
        retryAfterRevision.current = expectedRevision;
        return;
      }
      setCancelling(false);
      retryAfterRevision.current = undefined;
      notifyOperationFailure(onNotify, '模型可用性测试取消未完成', result.error);
    },
    [onNotify, test.id],
  );

  useEffect(() => {
    const previousRevision = retryAfterRevision.current;
    if (
      previousRevision === undefined ||
      state === 'completed' ||
      state === 'cancelled' ||
      revision === previousRevision
    ) {
      return;
    }
    retryAfterRevision.current = undefined;
    void cancelAtRevision(revision, true);
  }, [cancelAtRevision, revision, state]);

  const cancel = async () => {
    setCancelling(true);
    await cancelAtRevision(revision, false);
  };

  return (
    <section aria-label="模型可用性测试进度" className="model-operation-status">
      <div>
        <strong>
          可用性：
          {currentTest.result?.outcome === 'available'
            ? '可用'
            : currentTest.result?.outcome === 'unavailable'
              ? '不可用'
              : state === 'cancelled'
                ? '已取消'
                : state === 'completed'
                  ? '已完成'
                  : '测试中'}
        </strong>
        <span>
          {currentTest.stage ?? 'queued'} · Revision {revision}
        </span>
      </div>
      {currentTest.result === null || currentTest.result === undefined ? null : (
        <ModelAvailabilityCheckList result={currentTest.result} />
      )}
      {currentTest.result?.provider_error === null ||
      currentTest.result?.provider_error === undefined ? null : (
        <ProviderErrorSummary summary={currentTest.result.provider_error} />
      )}
      {state === 'queued' || state === 'running' ? (
        <>
          <p className="model-operation-cancellation-note">
            请求可能已经到达 Provider；取消无法撤回 Provider 已完成的处理或费用。
          </p>
          <button disabled={cancelling} onClick={() => void cancel()} type="button">
            {cancelling ? '取消中…' : '取消'}
          </button>
        </>
      ) : null}
    </section>
  );
}

export function ModelAvailabilityCheckList({ result }: { result: ModelAvailabilityTestResult }) {
  return (
    <dl className="availability-checks">
      {(
        [
          ['配置', result.config],
          ['网络', result.network],
          ['TLS', result.tls],
          ['HTTP', result.http],
          ['认证', result.authentication],
          ['模型生成', result.model_generation],
        ] as const
      ).map(([label, check]) => (
        <div key={label}>
          <dt>{label}</dt>
          <dd title={availabilityCheckCode(check)}>{availabilityCheckLabel(check)}</dd>
        </div>
      ))}
    </dl>
  );
}

export function ProviderErrorSummary({ summary }: { summary: ProviderErrorSummaryView }) {
  return (
    <div className="provider-error-summary" role="status">
      <strong>Provider 返回的错误信息（已脱敏）</strong>
      <p title={summary.issue_code}>
        {summary.provider_message ?? checkFailure(summary.issue_code, undefined)}
      </p>
    </div>
  );
}

const discoveryStateLabel = (
  state: ModelDiscoveryStartAccepted['state'],
  outcome: 'failed' | 'partial' | 'succeeded' | 'unsupported' | null | undefined,
): string => {
  if (state === 'cancelled') return '已取消';
  if (state !== 'completed') return state === 'queued' ? '排队中' : '请求中';
  switch (outcome) {
    case 'succeeded':
      return '已完成';
    case 'partial':
      return '部分完成';
    case 'unsupported':
      return '当前配置已禁用';
    default:
      return '失败';
  }
};

/** The checks' own failures in words; an unlisted code is described by its family. */
const CHECK_FAILURES: Record<string, string> = {
  'authentication.rejected': 'API Key 被拒绝，请检查后重新保存',
  'transport.authorization_failed': '认证没有通过，请检查 API Key',
  'provider.rate_limited': '服务商限流，请稍后再试',
  'provider.server_error': '服务商出错，请稍后再试',
  'provider.request_rejected': '服务商拒绝了请求，请检查模型名称与连接设置',
  'provider.invalid_response': '服务商的回复无法识别',
  'provider.visible_text_missing': '回复里没有可显示的文字',
  'network.timeout': '连接超时，请检查网络与代理设置',
  'network.transport_failed': '连接失败，请检查网络与代理设置',
  'network.proxy_unauthorized': '代理要求认证，请检查代理设置',
  'network.proxy_interrupted': '代理连接中断，请检查代理设置',
  'network.proxy_protocol_error': '代理回复异常，请检查代理设置',
  'network.tls_server_name_mismatch': '证书与地址不符，请检查 Base URL',
  'network.destination_blocked': '这个地址被网络规则拦截',
  'network.invalid_url': '地址无效，请检查 Base URL',
};
const CHECK_NOT_RUN: Record<string, string> = {
  blocked_by_previous_stage: '前一项没有通过',
  not_applicable_for_scheme: '不适用于这种连接方式',
  no_authentication_required: '这个连接无需认证',
  authentication_not_confirmed: '认证没有得到确认',
};

const checkFailure = (code: string, status: number | null | undefined): string => {
  const words =
    CHECK_FAILURES[code] ??
    (code.startsWith('network.')
      ? '网络请求失败，请检查网络与代理设置'
      : code.startsWith('provider.')
        ? '服务商返回了错误'
        : '这一项没有通过');
  return status === null || status === undefined ? words : `${words}（HTTP ${status}）`;
};

const availabilityCheckLabel = (result: ModelAvailabilityCheckResult): string => {
  switch (result.status) {
    case 'passed':
      return result.latency_ms === null || result.latency_ms === undefined
        ? '通过'
        : `通过 · ${result.latency_ms} ms`;
    case 'failed':
      return `失败 · ${checkFailure(result.error_code, result.provider_status)}`;
    case 'not_run':
      return `未执行 · ${CHECK_NOT_RUN[result.reason_code] ?? '没有执行'}`;
  }
};

/** The code behind a check, kept for diagnosis in the tooltip. */
const availabilityCheckCode = (result: ModelAvailabilityCheckResult): string | undefined =>
  result.status === 'failed'
    ? result.error_code
    : result.status === 'not_run'
      ? result.reason_code
      : undefined;

const notifyOperationFailure = (
  notify: (notification: NotificationInput) => void,
  title: string,
  error: { code: string; message: string },
) => {
  notify({
    dedupeKey: `${title}:${error.code}`,
    message: userMessage(error, MODEL_FAILURE_FALLBACK),
    title,
    tone: 'danger',
  });
};
