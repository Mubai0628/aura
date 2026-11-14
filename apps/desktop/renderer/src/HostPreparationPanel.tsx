import {
  HOST_ACTION_TITLE,
  hostPlanLines,
  hostPlanValidUntil,
} from '../../shared/host-preparation-text';
import { useSecurityControl } from './security-control-store';
import { hostOperationState } from './security/host-preparation-words';

type SecurityControl = ReturnType<typeof useSecurityControl>;

/** One isolation fact: a friendly name, the Windows feature it is when it is one, and its state. */
function Fact({
  label,
  technical,
  value,
  ready,
}: {
  label: string;
  technical?: string;
  value: string;
  ready: boolean | undefined;
}) {
  return (
    <div data-ready={ready === undefined ? undefined : String(ready)}>
      <dt>
        <span className="security-fact-dot" aria-hidden="true" />
        <span>{label}</span>
        {technical === undefined ? null : <small>{technical}</small>}
      </dt>
      <dd>{value}</dd>
    </div>
  );
}

/**
 * What isolation Windows gives Aura's tools, read as
 * a list of states, with the two maintenance actions together and removing Aura's settings apart.
 */
export function HostPreparationPanel({ security }: { security: SecurityControl }) {
  const summary = security.host?.summary;
  const operation = security.host?.active_operation;
  const state = (available: boolean | undefined, yes: string, no: string) =>
    summary === undefined ? '正在读取…' : available ? yes : no;
  const compatible =
    summary === undefined
      ? undefined
      : summary.system_drive_compatibility_ready && summary.nul_compatibility_ready;
  return (
    <section aria-labelledby="isolation-heading" className="settings-surface isolation-surface">
      <div className="section-heading-row compact">
        <div>
          <h3 id="isolation-heading">隔离与下载</h3>
        </div>
      </div>

      <dl className="security-facts">
        <Fact
          label="应用容器隔离"
          technical="AppContainer"
          value={state(summary?.appcontainer_available, '可用', '不可用')}
          ready={summary?.appcontainer_available}
        />
        <Fact
          label="进程组限制"
          technical="嵌套 Job"
          value={state(summary?.nested_jobs_available, '可用', '不可用')}
          ready={summary?.nested_jobs_available}
        />
        <Fact label="兼容环境" value={state(compatible, '就绪', '未完成')} ready={compatible} />
        <Fact label="隔离的下载" value={`${security.quarantine.length} 个`} ready={undefined} />
      </dl>

      {security.host?.active_operation === null ? (
        <div className="host-actions" aria-label="兼容环境操作">
          <button
            className="button is-small"
            onClick={() => void security.requestHostPreparation('prepare')}
            type="button"
          >
            配置兼容环境
          </button>
          <button
            className="button is-small"
            onClick={() => void security.requestHostPreparation('repair')}
            type="button"
          >
            修复兼容环境
          </button>
          <span className="host-actions-spacer" />
          <button
            className="button is-small button-danger"
            onClick={() => void security.requestHostPreparation('remove')}
            type="button"
          >
            移除 Aura 配置
          </button>
        </div>
      ) : null}

      {operation === null || operation === undefined ? null : (
        <div className="host-preparation-plan" role="status" aria-label="兼容环境状态">
          <strong>{HOST_ACTION_TITLE[operation.action]}</strong>
          <p>{hostOperationState(operation)}</p>
          <ul>
            {hostPlanLines(operation).map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
          {operation.state === 'prepared' || operation.state === 'awaiting_elevation' ? (
            <p className="plan-expiry">{hostPlanValidUntil(operation.expires_at)}</p>
          ) : null}
        </div>
      )}
    </section>
  );
}
