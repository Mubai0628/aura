import { ChosenSkillsLine } from '../composer-skills/SkillChoice';
import type { AgentInboxAction, AgentInboxProjection } from '@aura/contracts/generated';
import { useState, type ReactNode } from 'react';
import { AppIcon } from '../app-icons';
import { MarkdownBlocks } from '../markdown-body';
import { ToolIcon } from '../tool-icon';
import { userMessage } from './user-message';
import './inbox.css';

/** A queued message on one line: its words with the line breaks folded away. */
const oneLine = (text: string) => text.replace(/\s+/gu, ' ').trim();

/** A small action of the queue: an icon, its name for screen readers and in the tooltip. */
function QueueAction({
  label,
  icon,
  disabled,
  title,
  onClick,
  type = 'button',
}: {
  label: string;
  icon: ReactNode;
  disabled?: boolean;
  title?: string | undefined;
  onClick?: () => void;
  type?: 'button' | 'submit';
}) {
  return (
    <button
      aria-label={label}
      className="agent-inbox-action"
      disabled={disabled}
      onClick={onClick}
      title={title ?? label}
      type={type}
    >
      {icon}
    </button>
  );
}

/**
 * A message sent while a task runs waits here; it can be appended to the running task at
 * once, edited or deleted. "把队列消息框做小一些 …… 小区域按钮不要采用文字的了" —
 * a compact tray over the composer, about two thirds of its width: one line per message, opened in
 * place to read it all, and every action an icon whose name is in its tooltip. A message appended to the running task waits in the conversation instead (PendingAdditions).
 */
export function InboxPanel({
  inbox,
  activeTurn,
  onChanged,
}: {
  inbox: AgentInboxProjection;
  activeTurn: string | undefined;
  onChanged: () => void;
}) {
  const [editing, setEditing] = useState<string>();
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  // The message opened to be read in full.
  const [shown, setShown] = useState<string>();
  const mutate = async (action: AgentInboxAction) => {
    if (busy) return;
    setBusy(true);
    try {
      const result = await window.aura.v1.mutateAgentInbox({
        session_id: inbox.session_id,
        expected_revision: inbox.revision,
        action,
      });
      if (!result.ok) {
        setError(result.error.message);
        return;
      }
      setEditing(undefined);
      setError(undefined);
      onChanged();
    } catch {
      setError('操作结果未确认，请刷新队列核对。');
    } finally {
      setBusy(false);
    }
  };
  const queued = inbox.messages.filter((message) => message.destination.lane !== 'addition');
  if (!queued.length && !inbox.pause_reason) return null;
  return (
    <section className="agent-inbox" aria-label="消息队列">
      <header>
        <strong>
          {inbox.paused ? '下一轮已暂停' : '待处理'} · {queued.length}
        </strong>
        <QueueAction
          disabled={busy}
          icon={<ToolIcon name={inbox.paused ? 'play' : 'pause'} size={13} />}
          label={inbox.paused ? '继续队列' : '暂停队列'}
          onClick={() => void mutate({ action: inbox.paused ? 'resume' : 'pause' })}
        />
      </header>
      {inbox.pause_reason ? (
        <p role="alert">
          {userMessage(inbox.pause_reason, '队列已暂停，消息已保留。请确认后继续。')}
        </p>
      ) : null}
      <ol>
        {queued.map((message) => {
          const lane = '下一轮 · Normal';
          const open = shown === message.message_id;
          return (
            <li key={message.message_id} data-open={open || undefined}>
              <div className="agent-inbox-row">
                <span className="agent-inbox-lane" title={lane}>
                  <ToolIcon name="list-end" size={13} />
                  <span className="sr-only">{lane}</span>
                </span>
                {editing === message.message_id ? (
                  <form
                    className="agent-inbox-editor"
                    onSubmit={(event) => {
                      event.preventDefault();
                      void mutate({ action: 'edit', message_id: message.message_id, text });
                    }}
                  >
                    <textarea
                      aria-label="编辑排队消息"
                      // The editor opens to be typed in.
                      autoFocus
                      onChange={(event) => setText(event.target.value)}
                      rows={2}
                      value={text}
                    />
                    <QueueAction
                      disabled={busy || !text.trim()}
                      icon={<ToolIcon name="check" size={13} />}
                      label="保存"
                      type="submit"
                    />
                    <QueueAction
                      icon={<AppIcon name="close" size={13} />}
                      label="放弃编辑"
                      onClick={() => setEditing(undefined)}
                    />
                  </form>
                ) : (
                  <>
                    <button
                      aria-expanded={open}
                      className="agent-inbox-text"
                      onClick={() => setShown(open ? undefined : message.message_id)}
                      title={open ? '收起' : '展开全文'}
                      type="button"
                    >
                      {[
                        oneLine(message.text),
                        // The pictures and files sent with it, by name.
                        message.attachments?.length
                          ? `附件 ${message.attachments.map((a) => a.name).join('、')}`
                          : '',
                      ]
                        .filter(Boolean)
                        .join(' · ')}
                    </button>
                    <span className="agent-inbox-actions">
                      <QueueAction
                        disabled={busy}
                        icon={<ToolIcon name="pencil" size={13} />}
                        label="编辑"
                        onClick={() => {
                          setEditing(message.message_id);
                          setText(message.text);
                        }}
                      />
                      {activeTurn ? (
                        // A message with chosen Skills starts a new turn; it cannot join this one.
                        <QueueAction
                          disabled={busy || Boolean(message.skills?.length)}
                          icon={<ToolIcon name="corner-down-right" size={13} />}
                          label="立即追加"
                          onClick={() =>
                            void mutate({
                              action: 'append',
                              message_id: message.message_id,
                              turn_task_id: activeTurn,
                            })
                          }
                          title={
                            message.skills?.length
                              ? '带 Skill 的消息在下一轮开始时使用'
                              : '立即追加到当前任务'
                          }
                        />
                      ) : null}
                      <QueueAction
                        disabled={busy}
                        icon={<ToolIcon name="trash" size={13} />}
                        label="删除"
                        onClick={() =>
                          void mutate({ action: 'remove', message_id: message.message_id })
                        }
                      />
                    </span>
                  </>
                )}
              </div>
              {open && editing !== message.message_id ? (
                <div className="agent-inbox-full">
                  <MarkdownBlocks className="agent-message-text" text={message.text} />
                  <ChosenSkillsLine skills={message.skills} />
                </div>
              ) : null}
            </li>
          );
        })}
      </ol>
      {error ? <p role="alert">{userMessage(error)}</p> : null}
    </section>
  );
}
