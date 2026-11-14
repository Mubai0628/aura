import type { AgentInboxProjection, AgentPendingMessage } from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';
import { copyText } from '../clipboard';
import { ToolIcon } from '../tool-icon';
import { QuotedWords } from './quote-reply/QuotedWords';
import { userMessage } from './user-message';
import { connectionCause, failedAction } from './failure-text';
import './pending-additions.css';

/**
 * What the user appended to the running task
 * waits at its end as their message until the model takes it at its next step. 撤回 puts it back
 * in the queue over the composer, for the next turn.
 */
export function PendingAdditions({
  inbox,
  turnTaskId,
  onChanged,
}: {
  inbox: AgentInboxProjection;
  turnTaskId: string;
  onChanged: () => void;
}) {
  const waiting = inbox.messages.filter(
    (message) =>
      message.destination.lane === 'addition' && message.destination.turn_task_id === turnTaskId,
  );
  return waiting.map((message) => (
    <PendingAddition
      key={message.message_id}
      inbox={inbox}
      message={message}
      onChanged={onChanged}
    />
  ));
}

function PendingAddition({
  inbox,
  message,
  onChanged,
}: {
  inbox: AgentInboxProjection;
  message: AgentPendingMessage;
  onChanged: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [failure, setFailure] = useState<string>();
  const shown = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(shown.current), []);
  const copy = async () => {
    setFailure(undefined);
    const refused = await copyText(message.text, '没能复制消息，请重试。');
    if (refused) {
      setFailure(refused);
      return;
    }
    setCopied(true);
    window.clearTimeout(shown.current);
    shown.current = window.setTimeout(() => setCopied(false), 1600);
  };
  const revoke = async () => {
    if (busy) return;
    setBusy(true);
    setFailure(undefined);
    try {
      const result = await window.aura.v1.mutateAgentInbox({
        session_id: inbox.session_id,
        expected_revision: inbox.revision,
        action: { action: 'revoke', message_id: message.message_id },
      });
      if (!result.ok)
        setFailure(
          connectionCause(result.error)
            ? failedAction(
                result.error,
                '没能撤回',
                '这条消息可能仍会作为这一轮的输入；请看输入框上方的队列。',
              )
            : userMessage(
                result.error,
                '这条消息已经交给模型（或队列刚刚变化），没能撤回；它会作为这一轮的输入。',
              ),
        );
    } catch (reason) {
      const cause = connectionCause(reason) ?? '与 Aura Core 的连接出了问题';
      setFailure(
        `撤回的结果没有确认（${cause}），没有自动重发；请看输入框上方的队列，消息还在就可以再撤回。`,
      );
    } finally {
      setBusy(false);
      onChanged();
    }
  };
  const copyLabel = copied ? '已复制' : '复制消息';
  const files = message.attachments?.map((attachment) => attachment.name).join('、');
  return (
    <article className="agent-user-message agent-steer-message agent-pending-addition">
      <small>
        追加信息
        <span aria-label="等待模型接收" className="agent-pending-dots" role="status">
          <i />
          <i />
          <i />
        </span>
      </small>
      {message.text ? (
        <QuotedWords className="agent-message-text" text={message.text} words={(text) => text} />
      ) : null}
      {files ? <small>附件 {files}</small> : null}
      <div className="agent-user-actions">
        <div className="agent-reply-actions">
          <button
            aria-label={copyLabel}
            className="icon-button agent-reply-action"
            onClick={() => void copy()}
            title={copyLabel}
            type="button"
          >
            <ToolIcon name={copied ? 'check' : 'copy'} size={14} />
          </button>
          <button
            aria-label="撤回到队列"
            className="icon-button agent-reply-action"
            disabled={busy}
            onClick={() => void revoke()}
            title="撤回到队列"
            type="button"
          >
            <ToolIcon name="undo-2" size={14} />
          </button>
          {failure ? (
            <span className="agent-reply-failure" role="alert">
              {failure}
            </span>
          ) : null}
        </div>
      </div>
    </article>
  );
}
