import type {
  AgentTeamLimits,
  AgentTeamPermissions,
  AgentTeamSettingsView,
} from '@aura/contracts/generated';
import { useEffect, useId, useState } from 'react';
import { userMessage } from '../agent-session/user-message';
import { AgentTeamPermissionsForm } from './AgentTeamPermissionsForm';
import './agent-team-settings.css';

const UNREADABLE = '读不到多 Agent 的设置，请重试。';
const NOT_SAVED = '多 Agent 的设置没有保存，请重试。';

type Field = keyof AgentTeamLimits;
/** The fields as typed, and whether the tokens are left unlimited. */
type Draft = Record<Field, string> & { unlimited: boolean };

const FIELDS: readonly { field: Field; label: string; unit: string; step: number }[] = [
  { field: 'max_running', label: '同时运行的子 Agent', unit: '个', step: 1 },
  { field: 'token_budget', label: '每个子 Agent 的 token 上限', unit: 'token', step: 100_000 },
  { field: 'minutes_per_task', label: '每个任务的用时上限', unit: '分钟', step: 5 },
  { field: 'cpu_percent', label: '子 Agent 命令合计的 CPU 占比', unit: '%', step: 5 },
];

type Read = { view: AgentTeamSettingsView } | { problem: string };

const readSettings = async (): Promise<Read> => {
  try {
    const result = await window.aura.v1.readAgentTeamSettings();
    return result.ok ? { view: result.value } : { problem: userMessage(result.error, UNREADABLE) };
  } catch {
    return { problem: UNREADABLE };
  }
};

/** A limit's number; a token budget that is not set is none. */
const limitOf = (limits: AgentTeamLimits, field: Field): number | null => limits[field] ?? null;

const toDraft = (limits: AgentTeamLimits): Draft => ({
  max_running: String(limits.max_running),
  token_budget: limits.token_budget == null ? '' : String(limits.token_budget),
  minutes_per_task: String(limits.minutes_per_task),
  cpu_percent: String(limits.cpu_percent),
  unlimited: limits.token_budget == null,
});

/** The field's number when it is a whole number Core takes for it. */
const valueOf = (draft: Draft, view: AgentTeamSettingsView, field: Field): number | undefined => {
  const text = draft[field].trim();
  const number = text === '' ? Number.NaN : Number(text);
  const [least, most] = [limitOf(view.minimum, field) ?? 0, limitOf(view.maximum, field) ?? 0];
  return Number.isInteger(number) && number >= least && number <= most ? number : undefined;
};

/** Whether the field holds what Core takes: any number for unlimited tokens does. */
const takes = (draft: Draft, view: AgentTeamSettingsView, field: Field) =>
  (field === 'token_budget' && draft.unlimited) || valueOf(draft, view, field) !== undefined;

const parse = (draft: Draft, view: AgentTeamSettingsView): AgentTeamLimits | undefined => {
  if (!FIELDS.every(({ field }) => takes(draft, view, field))) return undefined;
  const value = (field: Field) => valueOf(draft, view, field) ?? 0;
  return {
    max_running: value('max_running'),
    token_budget: draft.unlimited ? null : value('token_budget'),
    minutes_per_task: value('minutes_per_task'),
    cpu_percent: value('cpu_percent'),
  };
};

const same = (one: AgentTeamLimits, other: AgentTeamLimits) =>
  FIELDS.every(({ field }) => limitOf(one, field) === limitOf(other, field));

/**
 * 设置 → 智能能力 → 多 Agent: how many sub-agents of a conversation work at once, the tokens (not
 * limited unless the user sets a number) and the minutes each may use on one task, and the
 * processor share their commands take together; then their permissions.
 */
export function AgentTeamSettings({ coreReady }: { coreReady: boolean }) {
  const [view, setView] = useState<AgentTeamSettingsView>();
  const [draft, setDraft] = useState<Draft>();
  const [problem, setProblem] = useState<string>();
  const [busy, setBusy] = useState(false);
  const baseId = useId();
  /** Shows what Core saved; limits typed and not saved yet stay when only the permissions went. */
  const show = (next: AgentTeamSettingsView, keepLimits = false) => {
    setView(next);
    if (!keepLimits) setDraft(toDraft(next.limits));
  };
  useEffect(() => {
    if (!coreReady) return;
    let live = true;
    void readSettings().then((read) => {
      if (!live) return;
      if ('view' in read) {
        setView(read.view);
        setDraft(toDraft(read.view.limits));
        setProblem(undefined);
      } else setProblem(read.problem);
    });
    return () => {
      live = false;
    };
  }, [coreReady]);

  const retry = async () => {
    const read = await readSettings();
    if ('view' in read) {
      show(read.view);
      setProblem(undefined);
    } else setProblem(read.problem);
  };
  const save = async (limits: AgentTeamLimits, permissions?: AgentTeamPermissions) => {
    if (view === undefined || busy) return;
    setBusy(true);
    try {
      const result = await window.aura.v1.setAgentTeamSettings({
        limits,
        ...(permissions === undefined ? {} : { permissions }),
        expected_revision: view.revision,
      });
      if (result.ok) {
        show(result.value, permissions !== undefined);
        setProblem(undefined);
        return;
      }
      // The reason stays on screen; changed elsewhere, the page shows what is saved now.
      setProblem(userMessage(result.error, NOT_SAVED));
      if (result.error.message.includes('agents.limits_changed')) {
        const again = await readSettings();
        if ('view' in again) show(again.view);
      }
    } catch {
      setProblem(NOT_SAVED);
    } finally {
      setBusy(false);
    }
  };

  const ready = view !== undefined && draft !== undefined && !busy;
  const chosen = view && draft ? parse(draft, view) : undefined;
  const wrong = view && draft ? FIELDS.find(({ field }) => !takes(draft, view, field)) : undefined;
  return (
    <div className="settings-page agent-team-settings">
      <div className="settings-content-heading">
        <div>
          <h2>多 Agent</h2>
        </div>
      </div>
      {coreReady ? null : (
        <p className="settings-status-line" role="status">
          Core 尚未就绪，多 Agent 设置暂不可用。
        </p>
      )}
      <form
        aria-label="子 Agent 的上限"
        className="settings-surface agent-team-settings-limits"
        onSubmit={(event) => {
          event.preventDefault();
          if (chosen !== undefined) void save(chosen);
        }}
      >
        {FIELDS.map(({ field, label, unit, step }) => {
          const id = `${baseId}-${field}`;
          const invalid = view !== undefined && draft !== undefined && wrong?.field === field;
          const tokens = field === 'token_budget';
          return (
            <div className="agent-team-settings-field" key={field}>
              <label htmlFor={id}>{label}</label>
              <span className="agent-team-settings-input">
                <input
                  aria-invalid={invalid || undefined}
                  disabled={!ready || (tokens && draft?.unlimited === true)}
                  id={id}
                  inputMode="numeric"
                  max={view === undefined ? undefined : (limitOf(view.maximum, field) ?? undefined)}
                  min={view === undefined ? undefined : (limitOf(view.minimum, field) ?? undefined)}
                  onChange={(event) =>
                    setDraft((current) =>
                      current === undefined ? current : { ...current, [field]: event.target.value },
                    )
                  }
                  step={step}
                  type="number"
                  value={draft?.[field] ?? ''}
                />
                <span aria-hidden="true">{unit}</span>
                {tokens ? (
                  <label className="agent-team-settings-unlimited">
                    <input
                      checked={draft?.unlimited ?? false}
                      disabled={!ready}
                      onChange={(event) =>
                        setDraft((current) =>
                          current === undefined
                            ? current
                            : { ...current, unlimited: event.target.checked },
                        )
                      }
                      type="checkbox"
                    />
                    不限
                  </label>
                ) : null}
              </span>
            </div>
          );
        })}
        {wrong !== undefined && view !== undefined ? (
          <p className="settings-status-line" role="alert">
            {`${wrong.label}：请输入 ${limitOf(view.minimum, wrong.field)} 到 ${limitOf(view.maximum, wrong.field)} 的整数。`}
          </p>
        ) : null}
        <div className="settings-card-actions">
          <button
            className="button is-primary is-small"
            disabled={
              !ready || view === undefined || chosen === undefined || same(chosen, view.limits)
            }
            type="submit"
          >
            保存
          </button>
          {view !== undefined && !same(view.limits, view.defaults) ? (
            <button
              className="button is-small"
              disabled={!ready}
              onClick={() => void save(view.defaults)}
              type="button"
            >
              恢复默认
            </button>
          ) : null}
        </div>
      </form>
      <AgentTeamPermissionsForm
        defaults={view?.permission_defaults}
        disabled={!ready}
        onSave={(permissions) => {
          if (view !== undefined) void save(view.limits, permissions);
        }}
        permissions={view?.permissions}
      />
      {problem ? (
        <p className="settings-status-line" role="alert">
          {problem}
          {view === undefined ? (
            <button className="button is-small" onClick={() => void retry()} type="button">
              重试
            </button>
          ) : null}
        </p>
      ) : null}
    </div>
  );
}
