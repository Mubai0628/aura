import type { ConfigRecoveryAction } from '@aura/contracts/generated';
import { useState } from 'react';
import { useConfigRecovery } from './useConfigRecovery';
import './settings-recovery.css';

const ACTIONS: Record<ConfigRecoveryAction, string> = {
  retry: '重试当前配置',
  restore_previous_valid: '恢复上一版有效配置',
  reset_to_default: '重置为默认值',
};
/** Why Core cannot offer an option (aura-config recovery codes), in the user's words. */
const REASONS: Record<string, string> = {
  'config.recovery.no_valid_previous_version': '没有可以恢复的上一版有效设置',
  'config.recovery.no_history': '还没有保存过历史版本',
  'config.recovery.no_current_snapshot': '当前设置读不出来',
  'config.recovery.no_consistent_bundle_version': '找不到彼此一致的一组历史版本',
  'config.recovery.action_not_offered': '这种情况不提供此方式',
  'config.recovery.action_unavailable': '此方式现在不可用',
  'config.recovery.scope_unknown': '影响范围无法确定',
  'config.recovery.held_by_unresolved_bundle': '还有一组设置修改没有处理完',
  'config.recovery.partial_application_rollback_failed': '上次只写入了一部分且没能撤回',
  'config.recovery.default_fails_validation': '默认值也通不过当前校验',
  'config.recovery.preview_stale': '设置已经变化，请重新查看预览',
  'config.recovery.action_not_available_for_bundle': '这组相关设置不能用此方式恢复',
};
const DOMAINS: Record<string, string> = {
  aura: '应用',
  models: '模型',
  tools: '工具',
  security: '权限与安全',
  hooks: '自动处理规则',
  mcp: 'MCP 工具连接',
  skills: '技能',
  commands: '命令',
  plugins: '插件',
  prompt: '提示词',
  browser: '浏览器',
  memory: '记忆',
  diagnostics: '诊断',
  update: '更新',
  context: '上下文',
  prompts: '提示词',
  provider: 'Provider',
  providers: 'Provider',
  ui: '界面',
  sessions: '会话设置',
  'settings-ui': '界面',
  'settings-prompt': '提示词',
  'settings-security': '权限与安全',
  'settings-browser': '浏览器',
  'settings-chrome': 'Chrome',
  'settings-memory': '记忆',
  'settings-diagnostics': '诊断',
  'settings-update': '更新',
};
const CAPABILITIES: Record<string, string> = {
  tool_policy: '新工具调用',
  model_authority: '新模型调用',
  security_authority: '需要授权的新操作',
  config_transaction_recovery: '依赖待恢复设置的新调用',
};
const label = (key: string) => DOMAINS[key] ?? key;

export function ConfigRecoveryPanel({ coreReady }: { coreReady: boolean }) {
  const recovery = useConfigRecovery(coreReady);
  const [chosen, setChosen] = useState<ConfigRecoveryAction>();
  const [allResetConfirmed, setAllResetConfirmed] = useState<string>();
  const { notice, plan, report, busy, message, unknown } = recovery;
  if (notice === undefined && message === undefined) return null;
  const affected = notice !== undefined && (notice.fault_codes.length > 0 || !notice.scope_known);
  if (!affected && report === undefined && message === undefined) return null;
  const selected =
    chosen !== undefined && plan?.options.some((o) => o.action === chosen && o.available)
      ? chosen
      : plan?.options.find((o) => o.available)?.action;
  const resettingAll = notice?.scope_known === false && selected === 'reset_to_default';
  const allConfirmed = plan !== undefined && allResetConfirmed === plan.preview_id;
  return (
    <section
      aria-labelledby="settings-recovery-title"
      aria-busy={busy}
      className="settings-recovery"
    >
      <h2 id="settings-recovery-title">{affected ? '设置需要恢复' : '设置恢复结果'}</h2>
      {affected ? (
        <>
          <p>
            {notice?.scope_known
              ? `受影响的设置：${notice.affected_domains.map(label).join('、')}`
              : '暂时无法确定全部受影响的设置，恢复预览会展示实际范围。'}
          </p>
          <p>
            暂停：
            {notice?.unavailable_capabilities
              .map((c) => CAPABILITIES[c] ?? '依赖这些设置的新调用')
              .join('、')}
            。查看和停止已有任务仍可使用。
          </p>
        </>
      ) : null}
      {message !== undefined ? <p role="alert">{message}</p> : null}
      {report !== undefined ? (
        <div role="status">
          <p>
            {report.outcomes.every((outcome) => outcome.applied)
              ? '设置已恢复。'
              : '部分设置仍未恢复，请查看当前状态。'}
          </p>
          {(report.unavailable_capabilities ?? []).length > 0 ? (
            <p>
              相关功能尚不可用：
              {report.unavailable_capabilities
                ?.map((c) => CAPABILITIES[c.capability] ?? '依赖这些设置的功能')
                .join('、')}
              。请检查其他受影响的设置或重试恢复。
            </p>
          ) : null}
        </div>
      ) : null}
      {plan === undefined ? (
        <div className="settings-recovery-actions">
          {affected && !unknown ? (
            <button
              disabled={busy || !coreReady}
              onClick={() => {
                setChosen(undefined);
                setAllResetConfirmed(undefined);
                void recovery.preview();
              }}
              type="button"
            >
              查看恢复选项
            </button>
          ) : null}
          <button
            disabled={busy || !coreReady}
            onClick={() => void recovery.refresh()}
            type="button"
          >
            检查当前状态
          </button>
        </div>
      ) : (
        <>
          <h3>确认恢复范围</h3>
          {resettingAll ? (
            <div role="note">
              <p>
                保存清单已损坏，无法可靠选择上一版。这会重置下面全部配置域，现有非保密设置将丢失，凭据绑定可能需要重新设置。
              </p>
              <label>
                <input
                  type="checkbox"
                  checked={allConfirmed}
                  disabled={busy}
                  onChange={(event) =>
                    setAllResetConfirmed(event.target.checked ? plan.preview_id : undefined)
                  }
                />
                我确认重置全部配置，现有非保密设置将丢失
              </label>
            </div>
          ) : null}
          <ul>
            {plan.domains.map((domain) => (
              <li key={domain.module}>
                {label(domain.module)}：修订 {domain.current_revision ?? '未知'} →{' '}
                {domain.next_revision}
                {domain.references_needing_rebinding.length > 0 ? (
                  <p>恢复后需重新关联：{domain.references_needing_rebinding.join('、')}</p>
                ) : null}
                {domain.loss.length > 0 ? <p>将失去：{domain.loss.join('；')}</p> : null}
              </li>
            ))}
          </ul>
          <fieldset disabled={busy}>
            <legend>选择恢复方式</legend>
            {plan.options.map((option) => (
              <label key={option.action}>
                <input
                  checked={selected === option.action}
                  disabled={!option.available}
                  name="configuration-recovery-action"
                  onChange={() => setChosen(option.action)}
                  type="radio"
                  value={option.action}
                />
                {ACTIONS[option.action]}
                {!option.available ? '（当前不可用）' : ''}
                {option.reason_code != null ? (
                  <small>原因：{REASONS[option.reason_code] ?? '此方式现在不可用'}</small>
                ) : null}
              </label>
            ))}
          </fieldset>
          <p>只处理上述设置。凭据、会话记录和工作区文件不会被删除；故障原件按原有保密规则保留。</p>
          <div className="settings-recovery-actions">
            <button
              disabled={
                busy || selected === undefined || !coreReady || (resettingAll && !allConfirmed)
              }
              onClick={() => {
                if (selected !== undefined && (!resettingAll || allConfirmed))
                  void recovery.apply(selected);
              }}
              type="button"
            >
              确认
              {resettingAll ? '重置全部配置' : selected === undefined ? '恢复' : ACTIONS[selected]}
            </button>
            <button disabled={busy} onClick={() => void recovery.refresh()} type="button">
              取消并查看当前状态
            </button>
          </div>
        </>
      )}
    </section>
  );
}
