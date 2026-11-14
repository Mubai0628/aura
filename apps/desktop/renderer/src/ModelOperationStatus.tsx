import type {
  ModelAvailabilityTestView,
  ModelAvailabilityTestResult,
  ModelDiscoveryStartAccepted,
  ProviderErrorSummaryView,
} from '@aura/contracts/generated';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { useScopedModelProjection } from './model-projection-store';
import type { NotificationInput } from './notification-center-store';
import {
  availabilityCheckLabel,
  checkFailure,
  discoveryOutcome,
} from './model-availability/operation-words';
import { modelFailureText } from './provider-connection/model-failure-words';

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
        <strong>模型目录：{discoveryOutcome(state, run)}</strong>
        <span>{discoveryStageLabel(run?.stage)}</span>
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
        <span>{testStageLabel(currentTest.stage)}</span>
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
        <div className={check.status === 'passed' ? undefined : 'is-wide'} key={label}>
          <dt>{label}</dt>
          <dd>{availabilityCheckLabel(check)}</dd>
        </div>
      ))}
    </dl>
  );
}

export function ProviderErrorSummary({ summary }: { summary: ProviderErrorSummaryView }) {
  return (
    <div className="provider-error-summary" role="status">
      <strong>服务商返回的错误</strong>
      <p>{checkFailure(summary.issue_code, undefined)}</p>
      {summary.provider_message ? <p>服务商原话（已脱敏）：{summary.provider_message}</p> : null}
    </div>
  );
}

/** Where a model list request is, in words. */
const discoveryStageLabel = (stage: string | null | undefined): string => {
  switch (stage) {
    case 'requesting':
      return '正在请求服务商';
    case 'parsing':
      return '正在解析';
    case 'enriching_catalog':
      return '正在补充目录信息';
    case 'completed':
      return '已完成';
    default:
      return '排队中';
  }
};

/** Which check an availability test is at, in words. */
const testStageLabel = (stage: string | null | undefined): string => {
  switch (stage) {
    case 'config':
      return '正在检查配置';
    case 'network':
      return '正在检查网络';
    case 'tls':
      return '正在检查 TLS';
    case 'http':
      return '正在检查 HTTP';
    case 'authentication':
      return '正在检查认证';
    case 'model_generation':
      return '正在试生成';
    case 'completed':
      return '检查完成';
    default:
      return '排队中';
  }
};

const notifyOperationFailure = (
  notify: (notification: NotificationInput) => void,
  title: string,
  error: { code: string; message: string },
) => {
  notify({
    dedupeKey: `${title}:${error.code}`,
    message: modelFailureText(error),
    title,
    tone: 'danger',
  });
};
