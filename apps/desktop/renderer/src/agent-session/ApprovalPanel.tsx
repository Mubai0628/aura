import { userMessage } from './user-message';
import { NetworkApprovalFacts } from './NetworkApprovalFacts';
import { DeletionTargets } from './DeletionTargets';
import { OutsideTargets } from './OutsideTargets';
import { ShellApprovalFacts } from './ShellApprovalFacts';
import { GitApprovalFacts } from './GitApprovalFacts';
import { PackageApprovalFacts } from './PackageApprovalFacts';
import { McpApprovalFacts } from './McpApprovalFacts';
import { MemoryApprovalFacts } from './MemoryApprovalFacts';
import { CommandApprovalFacts, WorkspaceInitApprovalFacts } from './WorkspaceInitApprovalFacts';
import {
  externalCapabilities,
  externalTargets,
  mcpResource,
  packageResource,
  workspaceCapabilities,
  workspaceTargets,
} from './approval-facts';
import type { ApprovalDecisionKind, ExecutionApprovalView } from '@aura/contracts/generated';
import { useEffect, useState } from 'react';
import { AppIcon } from '../app-icons';
import { approvalActionLabel } from './approval-action-label';

const labels: Record<ApprovalDecisionKind, string> = {
  allow_once: '仅此一次',
  allow_session: '本会话允许',
  allow_for_current_turn: '本轮允许',
  allow_workspace: '本工作区允许',
  deny: '拒绝',
};

const NATIVE_BACKEND_ID = 'windows-user-native-v1';

/**
 * Why this call is being asked about, in the terms the decision actually uses.
 *
 * The dialog previously showed the backend id, the guarantee list and the raw
 * `reasons` codes, which meant the explanation a person needed — "this will run as
 * your Windows user, outside the workspace" or "the restricted environment is not
 * available right now" — was only present as a technical code. The three cases are
 * genuinely different decisions, so they get three different sentences.
 */
type ApprovalKind = 'native' | 'restricted' | 'other';

function approvalKind(view: ExecutionApprovalView): ApprovalKind {
  if (view.backend.class === 'native' || view.backend.id === NATIVE_BACKEND_ID) return 'native';
  if (
    view.backend.class === 'restricted_user_offline' ||
    view.backend.class === 'restricted_user_online'
  ) {
    return 'restricted';
  }
  return 'other';
}

/** The allowed range, which is a different fact from how this call will run. */
function modeLabel(mode: ExecutionApprovalView['mode']): string {
  switch (mode) {
    case 'read_only':
      return '只读';
    case 'workspace_read_write':
      return '工作区读写';
    case 'full_access':
      return '完全访问';
    default:
      return String(mode);
  }
}

function shellLabel(shell: 'cmd' | 'powershell'): string {
  return shell === 'cmd' ? 'CMD' : 'PowerShell';
}

/**
 * The Shells one session answer covers, in the words the dialog uses.
 *
 * Read from the proposal rather than assumed: the covered range is a fact about the
 * answer, not something the Renderer derives from the tool that happened to be
 * approved first.
 */
function shellRangeLabel(shells: readonly ('cmd' | 'powershell')[]): string {
  const ordered = (['powershell', 'cmd'] as const).filter((shell) => shells.includes(shell));
  if (ordered.length === 0) return shellLabel('powershell');
  return ordered.map(shellLabel).join(' 与 ');
}

/** A deadline in the user's local time; the exact instant stays on the element. */
function Deadline({ at }: { at: string }) {
  const date = new Date(at);
  const shown = Number.isNaN(date.getTime())
    ? at
    : date.toLocaleString('zh-CN', {
        month: 'numeric',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        hour12: false,
      });
  return (
    <time dateTime={at} title={at}>
      {shown}
    </time>
  );
}

/** The first allow answer is the suggested one; refusing is shown as what it is. */
const decisionClass = (decision: ApprovalDecisionKind, first: boolean) =>
  decision === 'deny' ? 'button is-danger' : first ? 'button is-primary' : 'button';

export function ApprovalPanel({
  sessionId,
  approvalId,
  onChanged,
}: {
  sessionId: string;
  approvalId: string;
  onChanged: () => void;
}) {
  const [view, setView] = useState<ExecutionApprovalView>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let alive = true;
    void window.aura.v1
      .getAgentApproval({ session_id: sessionId, approval_id: approvalId })
      .then((result) => {
        if (!alive) return;
        if (result.ok) setView(result.value);
        else setError(result.error.message);
      })
      .catch(() => {
        if (alive) setError('无法读取审批。');
      });
    return () => {
      alive = false;
    };
  }, [sessionId, approvalId]);
  const decide = async (decision: ApprovalDecisionKind) => {
    if (!view || busy) return;
    setBusy(true);
    try {
      const result = await window.aura.v1.resolveAgentApproval({
        session_id: sessionId,
        approval_id: approvalId,
        expected_revision: view.approval.revision,
        decision,
      });
      if (!result.ok) setError(result.error.message);
      else setError(undefined);
      onChanged();
    } catch {
      setError('审批结果未确认，请核对任务状态。');
      onChanged();
    } finally {
      setBusy(false);
    }
  };
  const kind = view ? approvalKind(view) : 'other';
  const git =
    view?.backend.class === 'native' &&
    view.backend.id === 'agent-native-git-v1' &&
    ['git.commit', 'git.branch', 'git.sync', 'git.push'].includes(
      view.operation.content.action_kind,
    );
  const consent = view?.native_session_consent ?? undefined;
  const installed = view ? packageResource(view) : undefined;
  const mcp = view ? mcpResource(view) : undefined;
  const memory =
    view?.operation.content.resources.some((r) => r.scope_kind === 'memory_vault') ?? false;
  const init = view?.operation.content.action_kind === 'workspace_init.complete';
  const command = ['commands.save', 'commands.delete'].includes(
    view?.operation.content.action_kind ?? '',
  );
  const targets = view ? externalTargets(view) : [];
  const capabilities = view ? externalCapabilities(view) : [];
  // A workspace approval is the ordinary case the lower modes now reach — approving one named
  // target without changing the session mode — so it gets the same plain-language treatment
  // the external one has. Without it the dialog for a ReadOnly write would show only the
  // internal capability spelling, which the user cannot be asked to judge.
  const workspace = view ? workspaceTargets(view) : [];
  const workspaceCapabilityNames = view ? workspaceCapabilities(view) : [];
  // Only the capabilities actually asked for get a session reuse explanation: when a download's
  // network part is already covered, "allow for this session" records the file answer alone.
  const requested = view?.requested_capabilities ?? view?.operation.content.capabilities ?? [];
  const shell = view
    ? ['shell.powershell', 'shell.cmd', 'terminal.open', 'terminal.write'].includes(
        view.operation.content.action_kind,
      )
    : false;
  const network =
    !shell && (view?.operation.content.capabilities.includes('ordinary_network') ?? false);
  // A command whose text writes outside the workspace is asked each time.
  const outside =
    shell &&
    ((view?.operation.content.risks ?? []) as readonly string[]).includes(
      'outside_workspace_write',
    );
  const shellText = (action: Record<string, unknown>) =>
    typeof action.script === 'string'
      ? action.script
      : typeof action.input === 'string'
        ? action.input
        : '';
  const fileConsent =
    (workspace.length > 0 || targets.length > 0) &&
    requested.some((capability) =>
      /^(workspace|external)_file_(read|write|create|delete|move)$/.test(capability),
    );
  return (
    <section className="agent-approval" aria-label="当前任务审批">
      <header className="agent-approval-header">
        <span className="agent-approval-mark" aria-hidden="true">
          <AppIcon name="shield" size={15} />
        </span>
        <strong>操作需要确认</strong>
        {view ? (
          <span className="agent-approval-kind">
            {installed
              ? '运行已安装的工具'
              : approvalActionLabel(view.operation.content.action_kind)}
          </span>
        ) : null}
      </header>
      {view ? (
        <>
          <div className="agent-approval-body">
            {/* The card shows the model's own explanation; everything else is one click
                away under 查看详情. */}
            <p className="agent-approval-purpose">
              <span>模型说明</span>
              {view.operation.content.purpose || '模型没有说明这次操作的目的。'}
            </p>
            {((view.operation.content.risks ?? []) as readonly string[]).includes(
              'permanent_file_deletion',
            ) ? (
              // A deletion that skips the Recycle Bin is said before anything
              // else, and is allowed once at most.
              <>
                <p className="agent-approval-warning" role="alert">
                  这次操作会永久删除文件，不进回收站，无法恢复；只能允许这一次。
                </p>
                <DeletionTargets
                  action={view.operation.content.action as Record<string, unknown>}
                  actionKind={view.operation.content.action_kind}
                />
              </>
            ) : null}
            {outside ? (
              <>
                <p className="agent-approval-warning" role="alert">
                  这条命令会写工作区以外的位置；只能允许这一次。
                </p>
                <OutsideTargets
                  text={shellText(view.operation.content.action as Record<string, unknown>)}
                />
              </>
            ) : null}
            <details className="agent-approval-details">
              <summary>查看详情</summary>
              {memory ? (
                <MemoryApprovalFacts view={view} />
              ) : init ? (
                <WorkspaceInitApprovalFacts view={view} />
              ) : command ? (
                <CommandApprovalFacts view={view} />
              ) : git ? (
                <GitApprovalFacts view={view} />
              ) : shell ? (
                <ShellApprovalFacts view={view} />
              ) : installed ? (
                <PackageApprovalFacts view={view} resource={installed} />
              ) : mcp ? (
                <McpApprovalFacts view={view} server={mcp} />
              ) : kind === 'native' ? (
                <p className="agent-approval-mode">
                  将以当前 Windows
                  用户身份运行：权限不限于工作目录，也不受工作区文件范围限制。这是本次执行的执行方式，不代表允许范围变大。
                </p>
              ) : null}
              {kind === 'restricted' ? (
                <p className="agent-approval-mode">
                  将在受限账户中运行：文件范围限定为工作区，网络与凭据按受限策略处理。
                </p>
              ) : null}
              {workspace.length ? (
                <dl className="agent-approval-workspace">
                  <dt>工作区目标</dt>
                  <dd>
                    {workspace.map((target) => (
                      <span key={target.display}>
                        {target.display}（{target.scope}）
                      </span>
                    ))}
                  </dd>
                  <dt>请求能力</dt>
                  <dd>
                    {workspaceCapabilityNames.length
                      ? workspaceCapabilityNames.join('、')
                      : '未声明工作区文件能力'}
                  </dd>
                  <dt>确认期限</dt>
                  <dd>
                    请在 <Deadline at={view.approval.expires_at} />{' '}
                    前确认。批准后的可用范围以所点的按钮为准。
                  </dd>
                </dl>
              ) : null}
              {targets.length ? (
                <dl className="agent-approval-external">
                  <dt>外部目标</dt>
                  <dd>
                    {targets.map((target) => (
                      <span key={`${target.kind}:${target.display}`} data-scope={target.kind}>
                        {target.display}（{target.scope}）
                      </span>
                    ))}
                  </dd>
                  <dt>请求能力</dt>
                  <dd>{capabilities.length ? capabilities.join('、') : '未声明外部能力'}</dd>
                  <dt>确认期限</dt>
                  <dd>
                    请在 <Deadline at={view.approval.expires_at} />{' '}
                    前确认。批准后的可用范围以所点的按钮为准。
                  </dd>
                </dl>
              ) : null}
              {targets.length || workspace.length ? null : (
                <dl className="agent-approval-external">
                  <dt>确认期限</dt>
                  <dd>
                    请在 <Deadline at={view.approval.expires_at} />{' '}
                    前确认。批准后的可用范围以所点的按钮为准。
                  </dd>
                </dl>
              )}
              {network && !(git || installed || mcp || memory || init || command) ? (
                <NetworkApprovalFacts view={view} />
              ) : null}
              {init || command ? (
                <p>
                  批准只覆盖上面列出的这一次改动，不会成为本会话或工作区的后续写入授权
                  {command ? '；以后每次保存或删除命令都会再询问' : ''}。
                </p>
              ) : fileConsent ? (
                <p>
                  「本会话允许」覆盖显示的文件目标或目录范围；写入范围包含新建、覆盖、删除和重命名。
                  模式变化和 Core
                  重启后仍有效；撤销、到期或会话删除时失效。目标对象被替换须重新批准。 在 Aura
                  编辑器中保存不会使批准失效；被其他程序替换时须重新批准。 文件批准不授予联网权限。
                </p>
              ) : network || shell || git || installed || mcp ? null : consent === undefined ? (
                view.approval.allowed_decisions.includes('allow_session') ? (
                  <p>
                    「本会话允许」只覆盖这一类操作在本会话内的后续调用；每次调用仍会生成独立的操作身份与派发绑定。
                  </p>
                ) : (
                  <p>这类操作每次都会询问；批准只覆盖这一次调用。</p>
                )
              ) : (
                <p>
                  「本会话允许」会为普通 {shellRangeLabel(consent.shells)}{' '}
                  命令建立一份可复用的会话同意：本次 Core
                  运行中，同一会话的普通脚本不再逐次询问。撤销该同意、更改本会话权限或重启 Core
                  后失效；另一会话，或由插件、MCP 发起的调用不能借用。会话同意不表示 Native
                  具备操作系统级的工作区隔离。
                </p>
              )}
              {git || installed || mcp || memory || init || command || network ? null : (
                <pre>{JSON.stringify(view.operation.content.action, null, 2)}</pre>
              )}
              <dl className="agent-approval-technical">
                <dt>默认权限模式</dt>
                <dd>{modeLabel(view.mode)}</dd>
                <dt>本次执行方式</dt>
                <dd>
                  {view.backend.id} · {view.backend.class}
                </dd>
                <dt>隔离保障</dt>
                <dd>{view.backend.guarantees.join('、')}</dd>
                <dt>本次请求批准的能力</dt>
                <dd>
                  {(view.requested_capabilities ?? view.operation.content.capabilities).join('、')}
                </dd>
              </dl>
              {view.reasons.length ? (
                <div className="agent-approval-reasons">
                  <strong>技术原因</strong>
                  <ul>
                    {view.reasons.map((reason) => (
                      <li key={reason}>{reason}</li>
                    ))}
                  </ul>
                </div>
              ) : null}
            </details>
          </div>
          <div className="agent-approval-actions">
            {view.approval.allowed_decisions.map((decision, index) => (
              <button
                type="button"
                key={decision}
                className={decisionClass(
                  decision,
                  view.approval.allowed_decisions.findIndex((value) => value !== 'deny') === index,
                )}
                disabled={busy || view.approval.state !== 'pending'}
                aria-busy={busy ? true : undefined}
                onClick={() => void decide(decision)}
              >
                {labels[decision]}
              </button>
            ))}
          </div>
        </>
      ) : (
        <p className="agent-approval-loading" role="status">
          读取审批中…
        </p>
      )}
      {error ? (
        <p className="agent-approval-error" role="alert">
          {userMessage(error)}
        </p>
      ) : null}
    </section>
  );
}
