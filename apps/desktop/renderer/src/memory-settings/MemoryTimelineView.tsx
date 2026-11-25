import type {
  MemoryBatchRead,
  MemoryHistory,
  MemoryTimeline,
  MemoryTimelineQuery,
  MemoryVaultRequest,
} from '@aura/contracts/generated';
import { useId, useRef, useState } from 'react';
import type { useMemory } from './useMemory';

const ACTORS: Record<string, string> = {
  model_tool: '模型',
  automatic_maintenance: '自动整理',
  external: '外部编辑',
  settings_repair: '设置页',
  recovery: 'Aura 恢复',
};
const ACTIONS: Record<string, string> = {
  create: '新建',
  update: '修改',
  delete: '删除',
  repair: '修复',
  external_create: '在外部新建',
  external_edit: '在外部修改',
  external_delete: '在外部删除',
  interrupted_undo: '撤回中断的写入',
  interrupted_kept: '中断后已被改动，保留现状',
};
const FIRST: MemoryTimelineQuery = {
  author_session_id: null,
  since: null,
  until: null,
  category: null,
  before: null,
  limit: 20,
};

export function MemoryTimelineView({
  vault,
  call,
  recent,
}: {
  vault: MemoryVaultRequest;
  call: ReturnType<typeof useMemory>['call'];
  recent: MemoryHistory[];
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [page, setPage] = useState<MemoryTimeline>();
  const [selected, setSelected] = useState<string[]>([]);
  const [notes, setNotes] = useState<MemoryBatchRead>();
  const [busy, setBusy] = useState(false);
  const [issue, setIssue] = useState('');
  const last = useRef(FIRST);
  const liveRead = useRef(false);
  const entries =
    page?.entries ??
    recent.map((event, index) => ({
      cursor: `recent-${index}`,
      event,
      category: null,
      session_name_at_write: null,
    }));
  const load = async (query: MemoryTimelineQuery) => {
    liveRead.current = true;
    setBusy(true);
    setIssue('');
    setNotes(undefined);
    setSelected([]);
    try {
      setPage(await call('memory.vault.timeline', { vault, query }));
      last.current = query;
    } catch {
      liveRead.current = false;
      setIssue('暂时无法读取记忆时间线；可检查筛选条件后重试。');
    } finally {
      setBusy(false);
    }
  };
  const readSelected = async () => {
    const [first, ...rest] = selected;
    if (first === undefined) return;
    setBusy(true);
    setIssue('');
    try {
      setNotes(await call('memory.vault.read_many', { vault, ids: [first, ...rest] }));
    } catch {
      setIssue('未能读取所选记忆；请减少数量或刷新。');
    } finally {
      setBusy(false);
    }
  };
  return (
    <details
      onToggle={(event) => {
        const expanded = event.currentTarget.open;
        setOpen(expanded);
        if (expanded && !liveRead.current) void load(FIRST);
      }}
    >
      <summary>最近修改（{recent.length}）与时间线</summary>
      {open ? (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            const fields = new FormData(event.currentTarget);
            const text = (key: string) => String(fields.get(key) ?? '').trim() || null;
            const utc = (key: string) => {
              const value = text(key);
              return value ? new Date(`${value}Z`).toISOString() : null;
            };
            void load({
              ...FIRST,
              category: text('category'),
              author_session_id: text('author'),
              since: utc('from'),
              until: utc('to'),
            });
          }}
        >
          <label htmlFor={`${id}-author`}>作者会话 ID</label>{' '}
          <input id={`${id}-author`} name="author" spellCheck={false} />{' '}
          <label htmlFor={`${id}-category`}>类别</label>{' '}
          <input id={`${id}-category`} name="category" maxLength={64} />{' '}
          <label htmlFor={`${id}-from`}>开始时间（UTC）</label>{' '}
          <input id={`${id}-from`} name="from" type="datetime-local" />{' '}
          <label htmlFor={`${id}-to`}>截止时间（UTC，不含）</label>{' '}
          <input id={`${id}-to`} name="to" type="datetime-local" />{' '}
          <button type="submit" disabled={busy}>
            筛选
          </button>
        </form>
      ) : null}
      {issue ? <p role="alert">{issue}</p> : null}
      {busy ? <p role="status">正在读取…</p> : null}
      {page?.incomplete ? <p role="status">索引未完整，请稍后重试或重建索引。</p> : null}
      {page?.entries.length === 0 ? <p>没有符合条件的修改记录。</p> : null}
      <ul>
        {entries.map(({ cursor, event, category, session_name_at_write }) => (
          <li key={cursor}>
            <time dateTime={event.at}>{new Date(event.at).toLocaleString()}</time>
            {' · '}
            {ACTIONS[event.action] ?? event.action}
            {' · '}
            {ACTORS[event.actor] ?? event.actor}
            {session_name_at_write
              ? ` · ${session_name_at_write}`
              : event.session_id
                ? ' · 会话名未记'
                : event.actor === 'external'
                  ? ''
                  : ' · 来源未记'}
            {page && event.session_id ? (
              <button
                type="button"
                disabled={busy}
                onClick={() =>
                  void load({
                    ...last.current,
                    author_session_id: event.session_id ?? null,
                    before: null,
                  })
                }
              >
                按该作者筛选
              </button>
            ) : null}
            <br />
            {event.title || '（无标题）'} · <code>{event.path}</code>
            {category ? ` · ${category}` : ''}{' '}
            {event.id && category ? (
              <label>
                <input
                  type="checkbox"
                  checked={selected.includes(event.id)}
                  onChange={(change) => {
                    const note = event.id!;
                    setSelected((old) =>
                      change.target.checked
                        ? [...new Set([...old, note])]
                        : old.filter((item) => item !== note),
                    );
                  }}
                />
                选择 {event.title || event.path}
              </label>
            ) : null}
          </li>
        ))}
      </ul>
      {page?.before ? (
        <button
          type="button"
          disabled={busy}
          onClick={() => void load({ ...last.current, before: page.before ?? null })}
        >
          较早记录
        </button>
      ) : null}
      <button
        type="button"
        disabled={busy || selected.length === 0}
        onClick={() => void readSelected()}
      >
        读取所选记忆
      </button>
      {notes?.notes.map(({ note, path, provenance }) => (
        <details key={path}>
          <summary>{path}</summary>
          <p>
            创建者：
            {provenance?.creator?.session_name_at_write ??
              provenance?.creator?.session_id ??
              '来源未记'}
          </p>
          <pre className="peer-inbox-result">{note.body}</pre>
        </details>
      ))}
      {notes?.missing_ids.length ? <p>部分记忆已移动、删除或不可读，请刷新。</p> : null}
    </details>
  );
}
