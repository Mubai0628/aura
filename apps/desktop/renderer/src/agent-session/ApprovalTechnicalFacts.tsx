import type {
  BackendClass,
  EnforcementGuarantee,
  ExecutionApprovalView,
  ResolvedCapability,
} from '@aura/contracts/generated';
import { PERMISSION_MODE_NAMES } from '../security/permission-mode-names';
import { CAPABILITY_TEXT } from '../tool-packages/package-text';

/** The environments a call can run in, by the names the runtime card uses. */
const BACKENDS: Record<string, string> = {
  'windows-user-full-v1': '完全访问执行环境',
  'windows-user-native-v1': '本机执行环境',
  'windows-user-compat-v1': 'Shell 执行环境',
  'windows-user-readonly-v1': '只读执行环境',
  'windows-user-workspace-v1': '工作区执行环境',
  'agent-native-git-v1': 'Git 执行环境',
  'agent-native-package-v1': '工具包执行环境',
};

const CLASSES: Record<BackendClass, string> = {
  core_broker: '工具调度服务',
  restricted_user_offline: '受限账户（不联网）',
  restricted_user_online: '受限账户（可联网）',
  native: '本机执行环境',
  app_container: '受限进程',
  container: '容器',
};

const GUARANTEES: Record<EnforcementGuarantee, string> = {
  authorized_reads_only: '只读取批准的位置',
  credential_isolation: '凭据隔离',
  identity_isolation: '独立的系统身份',
  authorized_writes_only: '只写入批准的位置',
  offline_network: '不联网',
  exact_network_origins: '只访问批准的网址',
  child_process_control: '子进程受控',
  bounded_output: '输出有上限',
  read_only_user_files: '用户文件只读',
  direct_network_blocked_v1: '不能直接联网',
};

const CAPABILITIES: Record<ResolvedCapability, string> = {
  ...CAPABILITY_TEXT,
  workspace_file_move: '移动工作区文件',
  interactive_terminal: '使用交互终端',
  background_process: '运行后台进程',
  browser_read: '读取浏览器页面',
  browser_interaction: '操作浏览器页面',
  browser_sensitive_action: '浏览器里的敏感操作',
  configuration_read: '读取设置',
  configuration_write: '修改设置',
  registry_write: '修改注册表',
  system_environment_write: '修改系统环境变量',
  system_file_write: '修改系统文件',
  service_management: '管理系统服务',
  scheduled_task_management: '管理计划任务',
  driver_management: '管理驱动程序',
  windows_software_install: '安装 Windows 软件',
  elevated_operation: '需要管理员权限的操作',
  tool_registry_read: '读取工具列表',
  tool_ephemeral_exposure_write: '临时开放工具',
  agent_task_read: '读取任务记录',
  agent_task_write: '修改任务记录',
  memory_read: '读取记忆',
  memory_write: '修改记忆',
  memory_delete: '删除记忆',
  ordinary_network: '联网',
  chrome_read: '读取 Chrome 页面',
  chrome_interaction: '操作 Chrome 页面',
  chrome_sensitive_action: 'Chrome 里的敏感操作',
  external_extension_call: '调用外部扩展',
  plugin_development_install: '安装开发中的插件',
  desktop_read: '读取屏幕和程序窗口',
  desktop_input: '用鼠标键盘操作电脑',
};

/** Why Core asks; a reason without words here is left out. */
const REASONS: Record<string, string> = {
  'policy.approval_required': '当前权限模式要求确认',
  'backend.guarantees_changed': '执行环境的保障和上次批准时不同',
  'backend.native_explicit': '这次以你的 Windows 用户身份直接运行',
  'execution.isolation_reduced': '隔离程度低于受限环境',
};

const joined = (names: readonly (string | undefined)[]) =>
  [...new Set(names.filter((name): name is string => Boolean(name)))].join('、') || '无';

/** How an approved call runs, in words: its mode, environment, guarantees and capabilities. */
export function ApprovalTechnicalFacts({ view }: { view: ExecutionApprovalView }) {
  const reasons = view.reasons
    .map((reason) => REASONS[reason])
    .filter((reason): reason is string => Boolean(reason));
  return (
    <>
      <dl className="agent-approval-technical">
        <dt>默认权限模式</dt>
        <dd>{PERMISSION_MODE_NAMES[view.mode]}</dd>
        <dt>本次执行方式</dt>
        <dd>{BACKENDS[view.backend.id] ?? CLASSES[view.backend.class] ?? '其他执行环境'}</dd>
        <dt>隔离保障</dt>
        <dd>{joined(view.backend.guarantees.map((guarantee) => GUARANTEES[guarantee]))}</dd>
        <dt>本次请求批准的能力</dt>
        <dd>
          {joined(
            (view.requested_capabilities ?? view.operation.content.capabilities).map(
              (capability) => CAPABILITIES[capability as ResolvedCapability],
            ),
          )}
        </dd>
      </dl>
      {reasons.length ? (
        <div className="agent-approval-reasons">
          <strong>需要确认的原因</strong>
          <ul>
            {reasons.map((reason) => (
              <li key={reason}>{reason}</li>
            ))}
          </ul>
        </div>
      ) : null}
    </>
  );
}
