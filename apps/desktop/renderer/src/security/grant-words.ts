import type { ExternalGrantCapability, ResolvedCapability } from '@aura/contracts/generated';
import { answerMissing, connectionCause, refusedAction } from '../agent-session/failure-text';

/**
 * The grant lists' words: what a grant allows, and what a list read or a withdrawal that did not
 * happen says: why, what is in force, and what to do.
 */

/** What a directory or file grant allows. */
export const EXTERNAL_CAPABILITY_TEXT: Record<ExternalGrantCapability, string> = {
  read: '读取',
  write: '写入',
  create: '新建',
  delete: '删除',
  model_egress: '发给模型',
};

/** What a workspace grant allows. */
export const CAPABILITY_TEXT: Record<ResolvedCapability, string> = {
  workspace_file_read: '读取工作区文件',
  workspace_file_write: '修改工作区文件',
  workspace_file_create: '新建工作区文件',
  workspace_file_delete: '删除工作区文件',
  workspace_file_move: '移动或重命名工作区文件',
  external_file_read: '读取工作区外的文件',
  external_file_write: '修改工作区外的文件',
  external_file_create: '在工作区外新建文件',
  external_file_delete: '删除工作区外的文件',
  process_execute: '运行程序',
  interactive_terminal: '使用交互终端',
  background_process: '运行后台进程',
  git_read: '读取 Git 仓库',
  git_write: '修改 Git 仓库',
  git_remote_read: '从远程仓库拉取',
  git_remote_write: '推送到远程仓库',
  git_destructive: '不可撤回的 Git 操作',
  public_network_read: '读取公网',
  private_network_access: '访问内网或本机服务',
  data_egress: '向外发送数据',
  download_artifact: '下载文件',
  download_windows_executable: '下载 Windows 程序',
  external_service_read: '读取外部服务',
  external_service_mutation: '修改外部服务的数据',
  external_service_destructive_mutation: '删除外部服务的数据',
  browser_read: '读取网页',
  browser_interaction: '操作网页',
  browser_sensitive_action: '网页里的敏感操作',
  configuration_read: '读取配置',
  configuration_write: '修改配置',
  registry_write: '修改注册表',
  system_environment_write: '修改系统环境变量',
  system_file_write: '修改系统文件',
  service_management: '管理 Windows 服务',
  scheduled_task_management: '管理计划任务',
  driver_management: '管理驱动程序',
  windows_software_install: '安装 Windows 软件',
  elevated_operation: '以管理员权限运行',
  tool_registry_read: '读取工具列表',
  tool_ephemeral_exposure_write: '临时开放工具',
  agent_task_write: '派发子 Agent 任务',
  agent_task_read: '查看子 Agent 任务',
  memory_read: '读取记忆',
  memory_write: '写入记忆',
  memory_delete: '删除记忆',
  ordinary_network: '普通联网',
  chrome_read: '读取 Chrome 网页',
  chrome_interaction: '操作 Chrome 网页',
  chrome_sensitive_action: 'Chrome 里的敏感操作',
  external_extension_call: '调用外部扩展',
  plugin_development_install: '安装开发中的插件',
  desktop_read: '查看屏幕',
  desktop_input: '操作鼠标键盘',
};

/** A capability list in words; an unknown value is shown as it is. */
export const capabilityList = (
  capabilities: readonly string[],
  words: Readonly<Record<string, string>>,
): string => capabilities.map((each) => words[each] ?? each).join('、');

/** A grant list (`what`) that was not read: why, and to try again. */
export const grantsNotRead = (what: string, error: unknown): string =>
  refusedAction(error, `${what}没有读到`, '请点“重试”。');

/** A withdrawal refused because the grant changed first (withdrawn, expired or replaced). */
export const GRANT_STATE_CHANGED =
  '这项批准的状态刚变化（可能已撤销或到期），列表已重新读取，请按最新状态处理。';

const STATE_CHANGED = new Set(['permission.grant_state_changed', 'file.grant_state_changed']);

const codeOf = (error: unknown): string | undefined => {
  if (typeof error !== 'object' || error === null) return undefined;
  const code = (error as { code?: unknown }).code;
  return typeof code === 'string' ? code : undefined;
};

/**
 * A withdrawal (of `what`) that did not happen, after which the list is read again: why, and
 * `inForce`, what still holds. One whose answer did not come, or that failed inside Aura, may
 * have happened.
 */
export function revokeFailure(error: unknown, what: string, inForce: string): string {
  if (STATE_CHANGED.has(codeOf(error) ?? '')) return GRANT_STATE_CHANGED;
  if (answerMissing(error) || error instanceof Error) {
    const cause = connectionCause(error) ?? 'Aura 内部通信失败';
    return `撤销没有确认完成（${cause}），这项${what}可能仍然有效；列表已重新读取，请核对。`;
  }
  return refusedAction(error, `${what}没有撤销`, `${inForce}；列表已重新读取，请核对后再撤销。`);
}

/**
 * A Shell approval withdrawal that did not happen. One whose outcome is not known can be pressed
 * again: it is the same command, so it is not withdrawn twice.
 */
export function nativeRevokeFailure(error: unknown): string {
  if (STATE_CHANGED.has(codeOf(error) ?? '')) return GRANT_STATE_CHANGED;
  if (answerMissing(error) || error instanceof Error) {
    const cause = connectionCause(error) ?? 'Aura 内部通信失败';
    return `撤销没有确认完成（${cause}），这项 Shell 运行批准可能仍然有效。可以再点一次撤销，不会重复撤销。`;
  }
  return refusedAction(error, 'Shell 运行批准没有撤销', '它仍然有效；请稍后再试。');
}
