import type { ExecutionApprovalView } from '@aura/contracts/generated';

export function ShellApprovalFacts({ view }: { view: ExecutionApprovalView }) {
  const requested = view.requested_capabilities ?? view.operation.content.capabilities;
  const subject = view.operation.content.action_kind.startsWith('terminal.')
    ? 'Agent 终端'
    : 'Shell';
  return (
    <div className="agent-approval-mode">
      <p>{subject} 以你的 Windows 用户权限运行，不受系统级只读或目录限制。</p>
      <p>
        本次需批准：{requested.includes('process_execute') ? `运行 ${subject}` : ''}
        {requested.includes('process_execute') && requested.includes('ordinary_network')
          ? '、'
          : ''}
        {requested.includes('ordinary_network') ? `${subject} 联网` : ''}
        。运行批准与联网批准分别记录，可在会话授权管理中单独撤销。
      </p>
      <p>
        Shell
        联网通过代理环境变量提供软默认，未强制；程序可以忽略代理或改变环境。只读和工作区读写模式下，声明联网需单独批准；完全访问模式默认开启联网。
      </p>
      {/* Only where it can be chosen. */}
      {view.approval.allowed_decisions.includes('allow_session') ? (
        <p>
          「本会话允许」只限本会话的普通 PowerShell 与 CMD，也覆盖 Agent 终端。Core
          重启和模式切换后仍有效；撤销、到期或会话删除时失效。另一会话、插件和 MCP
          不能借用；运行批准不授予联网。
        </p>
      ) : null}
    </div>
  );
}
