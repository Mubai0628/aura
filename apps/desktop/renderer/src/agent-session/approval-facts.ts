import type { ExecutionApprovalView, ResourceDescriptor } from '@aura/contracts/generated';

/** The reach Core froze. A file is one object; a directory states its depth. */
function ordinaryScope(resource: { recursive: boolean; directory?: boolean }): string {
  if (resource.directory === true && resource.recursive) return '该目录及其下内容';
  if (resource.directory === true) return '该目录及直接子项（不含更深层内容）';
  return '仅该对象（不含其下内容）';
}

/** One external target as Core described it: the object, and how far the grant reaches. */
export type ExternalTarget = {
  display: string;
  scope: string;
  kind: 'ordinary' | 'system' | 'sensitive';
};

/**
 * The external objects this operation would touch, exactly as Core classified them.
 *
 * The dialog only ever reports these facts: the root, whether its contents are included,
 * and which capabilities are being asked for all come from the frozen operation, so a
 * model cannot widen its own request here.
 */
export function externalTargets(view: ExecutionApprovalView): ExternalTarget[] {
  const targets: ExternalTarget[] = [];
  for (const resource of view.operation.content.resources) {
    switch (resource.scope_kind) {
      case 'external_path':
        targets.push({
          display: resource.canonical_path.display,
          scope: ordinaryScope({
            recursive: resource.recursive,
            directory: resource.directory === true,
          }),
          kind: 'ordinary',
        });
        break;
      case 'system_path':
        targets.push({
          display: resource.canonical_path.display,
          scope: '系统位置',
          kind: 'system',
        });
        break;
      case 'sensitive_user_path':
        targets.push({
          display: resource.canonical_path.display,
          scope: '敏感数据位置',
          kind: 'sensitive',
        });
        break;
      default:
        break;
    }
  }
  return targets;
}

/** The external capabilities this operation asks for, in the user's words. */
export function externalCapabilities(view: ExecutionApprovalView): string[] {
  const names: Record<string, string> = {
    external_file_read: '读取外部文件',
    external_file_write: '写入外部文件',
    external_file_create: '新建外部文件',
    external_file_delete: '删除外部文件',
    external_file_move: '移动外部文件',
  };
  const asked = (view.requested_capabilities ?? view.operation.content.capabilities)
    .map((capability) => names[capability])
    .filter((name): name is string => Boolean(name));
  return [...new Set(asked)];
}

/** One workspace object this operation would touch, as Core resolved it. */
export type WorkspaceTarget = {
  display: string;
  scope: string;
};

/**
 * The workspace files and directories this operation would touch.
 *
 * A workspace approval is a real case that needs the same plain-language treatment the
 * external one already gets: the confirmed semantics let a ReadOnly session approve writing
 * one named target, and the user cannot judge that request if the dialog only shows the
 * internal capability spelling and a JSON blob. The facts come from the frozen operation's
 * own resources — the paths Core already canonicalized — so a model cannot widen its request
 * by writing something different into its arguments.
 *
 * A workspace descriptor names a target; it does not encode the object's kind or grant
 * recursive access. For example, a Shell working directory can name the root without
 * requesting file access to everything below it. Report only what the descriptor proves.
 */
export function workspaceTargets(view: ExecutionApprovalView): WorkspaceTarget[] {
  const targets: WorkspaceTarget[] = [];
  for (const resource of view.operation.content.resources) {
    switch (resource.scope_kind) {
      case 'workspace_path':
        targets.push({
          display: resource.canonical_relative_path,
          scope: '工作区内该目标',
        });
        break;
      case 'workspace_root':
        targets.push({
          display: '工作区根目录',
          scope: '具体操作范围见本次参数',
        });
        break;
      default:
        break;
    }
  }
  return targets;
}

/** The installed package a package tool call runs, when it is one. */
export type PackageResource = Extract<ResourceDescriptor, { scope_kind: 'tool_package' }>;
export function packageResource(view: ExecutionApprovalView): PackageResource | undefined {
  for (const resource of view.operation.content.resources)
    if (resource.scope_kind === 'tool_package') return resource;
  return undefined;
}

/** The MCP server one call names: an MCP call has exactly this one resource. */
export type McpServerResource = Extract<ResourceDescriptor, { scope_kind: 'mcp_server' }>;
export function mcpResource(view: ExecutionApprovalView): McpServerResource | undefined {
  for (const resource of view.operation.content.resources)
    if (resource.scope_kind === 'mcp_server') return resource;
  return undefined;
}

/** The workspace capabilities this operation asks for, in the user's words. */
export function workspaceCapabilities(view: ExecutionApprovalView): string[] {
  const names: Record<string, string> = {
    workspace_file_read: '读取工作区文件',
    workspace_file_write: '改写工作区文件',
    workspace_file_create: '新建工作区文件',
    workspace_file_delete: '删除工作区文件',
    workspace_file_move: '移动工作区文件',
  };
  const asked = (view.requested_capabilities ?? view.operation.content.capabilities)
    .map((capability) => names[capability])
    .filter((name): name is string => Boolean(name));
  return [...new Set(asked)];
}
