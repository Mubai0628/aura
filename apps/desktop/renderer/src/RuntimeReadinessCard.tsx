import type { BackendLifecycle, BackendStatus, CoreHealth } from '@aura/contracts/generated';

import { useRuntimeReadiness, refreshRuntimeReadiness } from './runtime-readiness-store';

const coreLabels: Record<CoreHealth, string> = {
  ready: '核心服务可用',
  starting: '核心服务正在启动',
  recovery_required: '核心数据需要恢复',
};
const backendLabels: Record<BackendLifecycle, string> = {
  dormant: '按需启动',
  starting: '正在启动',
  ready: '连接就绪',
  degraded: '暂不可用',
};
const names: Record<string, string> = {
  strict_appcontainer: '受限进程执行',
  egress_proxy: '受控网络代理',
  'windows-user-full-v1': '完全访问执行环境',
  'windows-user-native-v1': '本机执行环境',
  'windows-user-compat-v1': 'Shell 执行环境',
  'windows-user-readonly-v1': '只读执行环境',
  'windows-user-workspace-v1': '工作区执行环境',
  core_broker: '工具调度服务',
};

/** Why a service cannot run, by the reason Core records; prefixes cover a reason's variants. */
const CAUSES: readonly (readonly [string, string])[] = [
  ['core.not_ready', 'Core 还没有就绪'],
  ['compat.runtime_missing', 'Shell 运行环境还没有安装'],
  ['compat.runtime_unavailable', 'Shell 运行环境还没有准备好'],
  ['compat.preparation_failed', 'Shell 运行环境没有准备成功'],
  ['compat.runtime_verification_failed', 'Shell 运行环境校验没有通过'],
  ['runner.core_binding_mismatch', '执行安装绑定的不是当前这个 Core'],
  ['runner.core_binding_unreadable', '执行安装的绑定记录读不到'],
  ['runner.install_version', '执行安装的版本与 Aura 不一致'],
  ['runner.production_installation_required', '需要正式安装的 Aura'],
  ['native.caller_identity', '没能确认当前的 Windows 用户'],
  ['backend.enforcement_probe_required', '还没有检查过隔离能力'],
  ['backend.start_failed', '执行服务没能启动'],
  ['backend.', '执行服务还没有启动'],
];

const causeOf = (reason: string): string =>
  CAUSES.find(([prefix]) => reason.startsWith(prefix))?.[1] ??
  (reason.endsWith('.unsupported_or_unready')
    ? '这台电脑暂不支持或还没有准备好'
    : '原因记在 Core 日志里');

const SETTINGS = '设置 → 权限与沙箱';

/** A service's state, with why it cannot run and where it is prepared or repaired. */
const backendState = (backend: BackendStatus): string => {
  const { readiness } = backend;
  if (backend.backend === 'windows-user-compat-v1' && readiness.readiness === 'ready')
    return '可运行；目录与网络限制未强制';
  switch (readiness.readiness) {
    case 'ready':
      return backendLabels[backend.lifecycle];
    case 'unavailable':
      return readiness.reason === 'compat.preparing'
        ? '正在准备 Shell'
        : `暂不可用（${causeOf(readiness.reason)}）`;
    case 'setup_required':
      return `需要环境准备：到 ${SETTINGS} 完成准备后可用`;
    case 'repair_required':
      return `需要修复（${causeOf(readiness.reason)}）：到 ${SETTINGS} 修复后可用`;
  }
};

export function RuntimeReadinessCard() {
  const state = useRuntimeReadiness();
  return (
    <details aria-label="运行能力" className="runtime-readiness-card">
      <summary>
        <div>
          <strong>运行能力</strong>
          <p role="status" aria-label="运行能力状态">
            {state.loading
              ? '正在检查…'
              : (state.error ?? (state.value && coreLabels[state.value.core_health]))}
          </p>
        </div>
      </summary>
      {state.value && (
        <>
          <ul>
            {state.value.backends.map((backend) => (
              <li key={backend.backend}>
                <span>{names[backend.backend] ?? '其他执行服务'}</span>
                <span>{backendState(backend)}</span>
              </li>
            ))}
          </ul>
          {state.value.core_health === 'ready' && (
            <p>模型配置、文件和历史可用。执行能力在使用时检查权限与环境。</p>
          )}
          {state.value.backends.some((backend) => backend.lifecycle === 'degraded') && (
            <p>部分执行服务不可用。可在安全设置中查看环境准备状态。</p>
          )}
          {state.value.backends.some(
            (backend) =>
              backend.readiness.readiness === 'repair_required' &&
              backend.readiness.reason.startsWith('runner.core_binding_mismatch'),
          ) && (
            <p>
              执行安装绑定的是安装目录里已发布的 Core，与当前进程不是同一个文件。对 debug 目录执行
              Repair 不会改成当前这个程序。更换脚本或 Shell 不会修复。
            </p>
          )}
        </>
      )}
      <button type="button" className="readiness-refresh" onClick={refreshRuntimeReadiness}>
        刷新状态
      </button>
    </details>
  );
}
