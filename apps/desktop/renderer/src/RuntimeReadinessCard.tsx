import type { BackendLifecycle, CoreHealth } from '@aura/contracts/generated';

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
                <span>
                  {backend.backend === 'windows-user-compat-v1' &&
                  backend.readiness.readiness === 'ready'
                    ? '可运行；目录与网络限制未强制'
                    : backend.readiness.readiness === 'unavailable' &&
                        backend.readiness.reason === 'compat.preparing'
                      ? '正在准备 Shell'
                      : backend.readiness.readiness === 'setup_required'
                        ? '需要环境准备'
                        : backend.readiness.readiness === 'repair_required'
                          ? '需要修复'
                          : backendLabels[backend.lifecycle]}
                </span>
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
