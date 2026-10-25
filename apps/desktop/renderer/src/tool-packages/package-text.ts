import type {
  CanonicalToolPackageToolV1,
  CommandWorkingDirectoryV1,
  PackageCapabilityRequirementV1,
  PackageConformanceReasonV1,
  PackageConformanceStateV1,
  PackageWorkingDirectoryPreviewV1,
} from '@aura/contracts/generated';

/** User-facing labels. Kept beside the page so a new enum value fails the type check. */
export const CAPABILITY_TEXT: Record<PackageCapabilityRequirementV1, string> = {
  workspace_file_read: '读取工作区文件',
  workspace_file_write: '修改工作区文件',
  workspace_file_create: '创建工作区文件',
  workspace_file_delete: '删除工作区文件',
  external_file_read: '读取工作区外的文件',
  external_file_write: '修改工作区外的文件',
  external_file_create: '在工作区外创建文件',
  external_file_delete: '删除工作区外的文件',
  process_execute: '运行程序',
  git_read: '读取 Git 仓库',
  git_write: '修改 Git 仓库',
  git_remote_read: '读取 Git 远端',
  git_remote_write: '推送到 Git 远端',
  git_destructive: '可能丢失 Git 数据的操作',
  public_network_read: '读取公网',
  private_network_access: '访问内网或本机服务',
  data_egress: '向外发送数据',
  download_artifact: '下载文件',
  download_windows_executable: '下载可执行程序',
  external_service_read: '读取外部服务',
  external_service_mutation: '修改外部服务的数据',
  external_service_destructive_mutation: '删除或覆盖外部服务的数据',
};

export const STATE_TEXT: Record<PackageConformanceStateV1, string> = {
  not_run: '尚未验证',
  running: '正在验证',
  succeeded: '验证通过',
  failed: '验证未通过',
  cancelled: '验证已取消',
  interrupted: '验证中断，结果不明',
};

export const REASON_TEXT: Record<PackageConformanceReasonV1, string> = {
  completed: '通过',
  completed_result_unchecked: '通过；结果结构未在验证中检查，首次真实调用时校验',
  invalid_result: '结果不符合声明',
  invalid_protocol: '请求或协议不符合声明',
  invalid_artifact: '输出文件不符合声明',
  cancelled: '已取消',
  timed_out: '超时',
  output_limit: '输出超过上限',
  process_failed: '程序运行失败',
  transport_unconfirmed: '传输结果不明',
  cleanup_unconfirmed: '清理未确认',
  credential_reflected: '凭据出现在不该出现的位置',
  program_credential_unsupported: '运行程序的工具不能使用 Aura 保存的凭据',
};

const DIRECTORY_TEXT: Record<CommandWorkingDirectoryV1['type'], string> = {
  package_root: '工具包目录',
  workspace_root: '会话的工作区',
  resolved_directory_claim: '参数指定的工作区内目录',
};

export function directoryText(directory: PackageWorkingDirectoryPreviewV1): string {
  if (directory.type === 'not_applicable') return '不运行程序';
  const declared = DIRECTORY_TEXT[directory.declared.type];
  return directory.type === 'resolved' ? `${declared}：${directory.path}` : declared;
}

/** The exact HTTP origin a tool declares, or the slot the user must fill. */
export function httpTarget(tool: CanonicalToolPackageToolV1): string | undefined {
  if (tool.runtime.type !== 'declarative_http') return undefined;
  const target = tool.runtime.request.target;
  return target.type === 'fixed_https'
    ? `${target.origin}${target.base_path}`
    : `由你填写的服务地址（${target.endpoint_slot_key}）`;
}

export function fingerprintPrefix(hex: string): string {
  return hex.slice(0, 12);
}
