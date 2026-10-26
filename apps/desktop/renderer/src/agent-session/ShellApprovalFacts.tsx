import type { ExecutionApprovalView } from '@aura/contracts/generated';

export function ShellApprovalFacts({ view }: { view: ExecutionApprovalView }) {
  const requested = view.requested_capabilities ?? view.operation.content.capabilities;
  const subject = view.operation.content.action_kind.startsWith('terminal.')
    ? 'Agent 终端'
    : 'Shell';
  const asked = [
    requested.includes('process_execute') ? `运行 ${subject}` : '',
    requested.includes('ordinary_network') ? `${subject} 联网` : '',
    requested.includes('download_artifact') ? '下载' : '',
  ].filter(Boolean);
  // A session answer for a download alone covers later downloads, not running in read-only mode.
  const downloadOnly =
    requested.includes('download_artifact') && !requested.includes('process_execute');
  return (
    <div className="agent-approval-mode">
      <p>{subject} 以你的 Windows 用户权限运行，不受系统级只读或目录限制。</p>
      <p>
        本次需批准：{asked.join('、')}
        。运行、联网与下载的批准分别记录，可在会话授权管理中单独撤销。
      </p>
      <p>
        Shell
        联网通过代理环境变量提供软默认，未强制；程序可以忽略代理或改变环境。只读模式下声明联网需单独批准，工作区读写和完全访问模式不问联网。完全访问以外，下载文件、安装依赖或拉取代码的命令每个会话问一次，删除文件或写到工作区以外的命令每次都问；脚本和程序内部做什么看不到。
      </p>
      {/* Only where it can be chosen. */}
      {view.approval.allowed_decisions.includes('allow_session') ? (
        downloadOnly ? (
          <p>
            「本会话允许」覆盖本会话之后的下载命令，PowerShell、CMD 与 Agent 终端都算。Core
            重启和模式切换后仍有效；撤销、到期或会话删除时失效。另一会话、插件和 MCP 不能借用。
          </p>
        ) : (
          <p>
            「本会话允许」只限本会话的普通 PowerShell 与 CMD，也覆盖 Agent 终端。Core
            重启和模式切换后仍有效；撤销、到期或会话删除时失效。另一会话、插件和 MCP
            不能借用；运行批准不授予联网。
          </p>
        )
      ) : null}
    </div>
  );
}
