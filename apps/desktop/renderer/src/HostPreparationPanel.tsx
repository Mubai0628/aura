import { useSecurityControl } from './security-control-store';

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
        <div className="host-actions" aria-label="Host Preparation 操作">
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

      {security.host?.active_operation !== null && security.host?.active_operation !== undefined ? (
        <div className="host-preparation-plan" role="status" aria-label="Host Preparation 状态">
          <strong>
            {security.host.active_operation.action === 'remove'
              ? '移除 Host Preparation'
              : security.host.active_operation.action === 'repair'
                ? '修复 Host Preparation'
                : '配置 Host Preparation'}
          </strong>
          <p>
            当前状态：{security.host.active_operation.state}。确认与管理员权限请求由受信任的
            Electron Main 原生对话框处理。
          </p>
          <ul>
            {security.host.active_operation.add_all_application_packages_ace ? (
              <li>添加 ALL APPLICATION PACKAGES 精确兼容 ACE</li>
            ) : null}
            {security.host.active_operation.add_all_restricted_application_packages_ace ? (
              <li>添加 ALL RESTRICTED APPLICATION PACKAGES 精确兼容 ACE</li>
            ) : null}
            {security.host.active_operation.update_null_device_security ? (
              <li>更新或恢复 NUL 设备兼容安全描述符</li>
            ) : null}
          </ul>
          <p className="plan-expiry">
            到期时间：{new Date(security.host.active_operation.expires_at).toLocaleString()}
          </p>
        </div>
      ) : null}
    </section>
  );
}
