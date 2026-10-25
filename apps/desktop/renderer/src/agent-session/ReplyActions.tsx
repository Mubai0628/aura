import type { AgentMessageTextSource } from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';
import { copyText } from '../clipboard';
import { ToolIcon } from '../tool-icon';
import { readWholeText } from './whole-text';
import './reply-actions.css';

/**
 * 复制 copies
 * the whole text as written, read to its end first. 新建分支 shows only where the caller can branch
 * (under a finished turn's answer); nothing passes `onBranch` yet. Used under a turn's answer and beside a user's message
 * (`what="message"`), not under the notes between tools.
 */
export function ReplyActions({
  sessionId,
  source,
  totalBytes,
  onBranch,
  branching = false,
  what = 'reply',
}: {
  sessionId: string;
  source: AgentMessageTextSource;
  totalBytes: number;
  /** Offered only where a branch can start: under a finished turn's answer. */
  onBranch?: (() => void) | undefined;
  branching?: boolean;
  /** A model's reply, or a user's message. */
  what?: 'reply' | 'message';
}) {
  const [copied, setCopied] = useState(false);
  const [failure, setFailure] = useState<string>();
  const shown = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(shown.current), []);
  const failed = what === 'message' ? '没能复制消息，请重试。' : '没能复制回复，请重试。';
  const copy = async () => {
    setFailure(undefined);
    let text: string;
    try {
      text = await readWholeText(sessionId, source, totalBytes);
    } catch {
      setFailure(failed);
      return;
    }
    const refused = await copyText(text, failed);
    if (refused) {
      setFailure(refused);
      return;
    }
    setCopied(true);
    window.clearTimeout(shown.current);
    shown.current = window.setTimeout(() => setCopied(false), 1600);
  };
  const copyLabel = copied ? '已复制' : what === 'message' ? '复制消息' : '复制';
  return (
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
      {onBranch ? (
        <button
          aria-label="新建分支"
          className="icon-button agent-reply-action"
          disabled={branching}
          onClick={onBranch}
          title="新建分支：一个新会话，带着到这一轮为止的全部对话"
          type="button"
        >
          <ToolIcon name="git-branch" size={14} />
        </button>
      ) : null}
      {failure ? (
        <span className="agent-reply-failure" role="alert">
          {failure}
        </span>
      ) : null}
    </div>
  );
}

/**
 * A user's message has its 复制 just left of it,
 * shown while the pointer or the keyboard is on the message, so it takes no room of its own.
 */
export function UserCopy({
  sessionId,
  eventId,
  totalBytes,
}: {
  sessionId: string;
  eventId: string;
  totalBytes: number;
}) {
  return (
    <div className="agent-user-actions">
      <ReplyActions
        sessionId={sessionId}
        source={{ kind: 'user', event_id: eventId }}
        totalBytes={totalBytes}
        what="message"
      />
    </div>
  );
}
