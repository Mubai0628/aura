import { userMessage } from './agent-session/user-message';
import type {
  SessionToolActivityProjection,
  ToolCallActivitySummaryView,
  ToolCallActivityHistoryViewV1,
} from '@aura/contracts/generated';
import { useState } from 'react';

import { AppIcon } from './app-icons';
import { toolCallStateLabel } from './tool-call-state-label';

export function SessionToolActivityCard({
  activity,
}: {
  activity: SessionToolActivityProjection | undefined;
}) {
  if (
    activity === undefined ||
    (activity.active_batches.length === 0 && activity.recent_calls.length === 0)
  ) {
    return null;
  }

  return (
    <SessionToolActivityCardContent
      activity={activity}
      key={`${activity.session_id}:${activity.older_activity_cursor ?? ''}`}
    />
  );
}

function SessionToolActivityCardContent({ activity }: { activity: SessionToolActivityProjection }) {
  const [history, setHistory] = useState<ToolCallActivityHistoryViewV1[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(
    activity.older_activity_cursor ?? null,
  );
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const expectedSessionId = activity.session_id;

  return (
    <section aria-label="Tool 活动" className="session-tool-activity-card">
      <header>
        <span className="session-tool-activity-icon" aria-hidden="true">
          <AppIcon name="code" />
        </span>
        <div>
          <strong>Tool 活动</strong>
          <p>
            {activity.active_batches.length} 个活动 Batch · {activity.recent_calls.length}{' '}
            条近期调用
          </p>
        </div>
      </header>
      <ol>
        {activity.recent_calls.slice(0, 6).map((call) => (
          <ToolCallRow call={call} key={call.tool_call_id} />
        ))}
        {history.map((item) => (
          <ToolCallRow call={item.call} key={item.call.tool_call_id} />
        ))}
      </ol>
      {nextCursor === null ? null : (
        <button
          className="session-tool-activity-more"
          disabled={loading}
          onClick={() => void loadOlderActivity()}
          type="button"
        >
          {loading ? '正在加载…' : '加载更早活动'}
        </button>
      )}
      {loadError === null ? null : (
        <p className="session-tool-activity-error" role="alert">
          {loadError}
        </p>
      )}
    </section>
  );

  async function loadOlderActivity(): Promise<void> {
    if (nextCursor === null || loading) return;
    setLoading(true);
    setLoadError(null);
    try {
      const result = await window.aura.v1.readToolActivityPage({ cursor: nextCursor });
      if (!result.ok) {
        setLoadError(userMessage(result.error, '工具活动读取失败，请重试。'));
        return;
      }
      if (result.value.session_id !== expectedSessionId) {
        setLoadError('Core 返回了不匹配的会话活动。');
        return;
      }
      setHistory((current) => [...current, ...result.value.items]);
      setNextCursor(result.value.next_cursor ?? null);
    } finally {
      setLoading(false);
    }
  }
}

function ToolCallRow({ call }: { call: ToolCallActivitySummaryView }) {
  return (
    <li>
      <span className={`tool-call-dot state-${call.status}`} aria-hidden="true" />
      <span>
        <strong>{call.display_name}</strong>
        <small>{toolCallStateLabel(call)}</small>
      </span>
      <time dateTime={call.completed_at ?? call.started_at ?? undefined}>
        {formatActivityTime(call.completed_at ?? call.started_at)}
      </time>
    </li>
  );
}

const formatActivityTime = (value: string | null | undefined): string => {
  if (value === undefined || value === null) return '—';
  const parsed = new Date(value);
  if (Number.isNaN(parsed.valueOf())) return '—';
  return new Intl.DateTimeFormat('zh-CN', { hour: '2-digit', minute: '2-digit' }).format(parsed);
};
