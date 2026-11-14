import { useEffect, useState, type ReactNode } from 'react';
import { ago } from './relative-time';
import { useChangeSummary, type ChangeSummary, type ChangeSummaryGap } from './useChangeSummary';
import { useRecentActions } from './useRecentActions';
import './activity-summary.css';

export interface ActivitySummaryProps {
  /** The conversation's mode and its controls, shown with its state. */
  children?: ReactNode;
  /** The programs the model is controlling now, right after the conversation's own part. */
  computer?: ReactNode;
  /** The conversation's sub-agents, after what it controls. */
  agents?: ReactNode;
  /** The conversation's Goal, after the conversation's own part. */
  goal?: ReactNode;
  /** The model's task list, after the conversation's own part. */
  todo?: ReactNode;
  workspaceId: string;
  sessionId: string;
  running: boolean;
  /** Waits for the user — an approval, an answer, a secure input — as its sidebar row shows. */
  waiting?: boolean | undefined;
  /** When the conversation's last turn ended. */
  lastEndedAt?: string | undefined;
  /** Changes when a turn starts or ends: what was done and changed is read again. */
  refreshKey: string;
  onOpenReview: () => void;
}

/** A clock that ticks every half minute, so "N 分钟前" stays true while the card is open. */
function useHalfMinute(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, []);
  return now;
}

const GAPS: Record<ChangeSummaryGap, string> = {
  'no-git': '未检测到 Git，无法汇总改动',
  'too-old': '本机 Git 版本低于 2.40，无法汇总改动',
  invalid: '无法验证本机 Git，无法汇总改动',
  'not-repository': '这个工作区不是 Git 仓库，没有可汇总的改动',
  unreadable: '暂时读不到 Git 状态，稍后会再读',
};

/** The uncommitted changes in a line: files, then lines added (green) and deleted (red). */
function Changes({ changes }: { changes: ChangeSummary | undefined }) {
  if (!changes || changes.state === 'reading') return <>正在读取…</>;
  if (changes.state === 'unavailable') return <>{GAPS[changes.reason]}</>;
  if (!changes.files) return <>没有未提交的改动</>;
  return (
    <>
      {changes.files}
      {changes.partial ? '+' : ''} 个文件 · <span className="activity-added">+{changes.added}</span>{' '}
      <span className="activity-deleted">−{changes.deleted}</span>
      {changes.uncounted ? `（另有 ${changes.uncounted} 个新文件未计行数）` : ''}
    </>
  );
}

/**
 * Three parts — the
 * conversation (its state, and its mode as `children`), what is uncommitted (the review's own
 * totals, added lines green and deleted red, a way to the review), and its last three tool calls.
 */
export function ActivitySummary({
  children,
  computer,
  agents,
  goal,
  todo,
  workspaceId,
  sessionId,
  running,
  waiting = false,
  lastEndedAt,
  refreshKey,
  onOpenReview,
}: ActivitySummaryProps) {
  const now = useHalfMinute();
  const changes = useChangeSummary(workspaceId, sessionId, refreshKey);
  const actions = useRecentActions(sessionId, refreshKey);
  const ended = ago(lastEndedAt, now);
  return (
    <>
      <section aria-label="会话" className="activity-section">
        <h4 className="activity-section-title">会话</h4>
        <div className="activity-row">
          <span className="activity-label">状态</span>
          <span
            className="activity-value"
            data-running={(running && !waiting) || undefined}
            data-waiting={waiting || undefined}
          >
            {waiting
              ? '要你处理'
              : running
                ? '正在运行'
                : ended
                  ? `空闲 · 上次完成于 ${ended}`
                  : '空闲'}
          </span>
        </div>
        {children}
      </section>
      {computer}
      {agents}
      {goal}
      {todo}
      <section aria-label="未提交改动" className="activity-section">
        <h4 className="activity-section-title">未提交改动</h4>
        <div className="activity-row">
          <span className="activity-value">
            <Changes changes={changes} />
          </span>
          {changes?.state === 'read' && changes.files > 0 ? (
            <button className="activity-link" onClick={onOpenReview} type="button">
              查看审查
            </button>
          ) : null}
        </div>
      </section>
      {actions.length ? (
        <section aria-label="最近操作" className="activity-section">
          <h4 className="activity-section-title">最近操作</h4>
          <ol aria-label="最近操作">
            {actions.map((action) => (
              <li data-failed={action.failed || undefined} key={action.id}>
                <span className="activity-recent-title">{action.title}</span>
                {action.target ? (
                  <span className="activity-recent-target">{action.target}</span>
                ) : null}
                {action.state ? (
                  <span className="activity-recent-state">{action.state}</span>
                ) : null}
                <time>{ago(action.at, now) ?? ''}</time>
              </li>
            ))}
          </ol>
        </section>
      ) : null}
    </>
  );
}
