import { userMessage } from '../agent-session/user-message';
import type { ApprovalDecisionKind, ApprovalView } from '@aura/contracts/generated';
import { useState } from 'react';
import * as Collapsible from '@radix-ui/react-collapsible';
import { AppIcon } from '../app-icons';
import type { useSecurityControl } from '../security-control-store';
import './security-ui.css';

const LABELS: Record<ApprovalDecisionKind, string> = {
  allow_for_current_turn: '本轮允许',
  allow_once: '仅此一次',
  allow_session: '本会话允许',
  allow_workspace: '本工作区允许',
  deny: '拒绝',
};

export function SessionApprovalTray({
  sessionId,
  security,
}: {
  sessionId: string;
  security: Pick<ReturnType<typeof useSecurityControl>, 'approvals' | 'resolveApproval'>;
}) {
  const approvals = security.approvals.filter((item) => item.session_id === sessionId);
  const [pending, setPending] = useState<string>();
  const [error, setError] = useState<string>();
  if (approvals.length === 0) return null;
  const resolve = async (approval: ApprovalView, decision: ApprovalDecisionKind) => {
    if (pending !== undefined) return;
    setPending(approval.id);
    setError(undefined);
    try {
      await security.resolveApproval(approval, decision);
    } catch (reason) {
      setError(userMessage(reason, '审批未提交，请检查连接。'));
    } finally {
      setPending(undefined);
    }
  };
  return (
    <Collapsible.Root className="session-approval-tray">
      <Collapsible.Trigger className="approval-tray-trigger">
        <AppIcon name="shield" size={16} />
        <span>{approvals.length} 项操作等待你确认</span>
        <AppIcon name="chevron-down" size={15} />
      </Collapsible.Trigger>
      <Collapsible.Content className="approval-tray-content">
        <ul className="approval-list">
          {approvals.map((approval) => (
            <li key={approval.id}>
              <strong>{approval.title}</strong>
              <p>{approval.actual_operation_summary}</p>
              <div className="approval-actions">
                {approval.allowed_decisions.map((decision) => (
                  <button
                    type="button"
                    disabled={pending !== undefined}
                    key={decision}
                    className={decision === 'deny' ? 'button-danger' : undefined}
                    onClick={() => void resolve(approval, decision)}
                  >
                    {pending === approval.id ? '正在提交…' : LABELS[decision]}
                  </button>
                ))}
              </div>
            </li>
          ))}
        </ul>
        {error === undefined ? null : <p role="alert">{error}</p>}
      </Collapsible.Content>
    </Collapsible.Root>
  );
}
