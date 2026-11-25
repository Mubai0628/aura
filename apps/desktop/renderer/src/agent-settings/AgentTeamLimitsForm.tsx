import type { AgentTeamLimits, AgentTeamSettingsView } from '@aura/contracts/generated';
import { useId } from 'react';
import { FIELDS, limitOf, parse, same, takes, type Draft } from './team-limits-form';

export function AgentTeamLimitsForm({
  view,
  draft,
  disabled,
  onDraft,
  onSave,
  onDefault,
}: {
  view: AgentTeamSettingsView | undefined;
  draft: Draft | undefined;
  disabled: boolean;
  onDraft: (draft: Draft) => void;
  onSave: (limits: AgentTeamLimits) => void;
  onDefault: () => void;
}) {
  const baseId = useId();
  const chosen = view && draft ? parse(draft, view) : undefined;
  const wrong = view && draft ? FIELDS.find(({ field }) => !takes(draft, view, field)) : undefined;
  return (
    <form
      aria-label="子 Agent 的上限"
      className="settings-surface agent-team-settings-limits"
      noValidate
      onSubmit={(event) => {
        event.preventDefault();
        if (!disabled && chosen !== undefined) onSave(chosen);
      }}
    >
      {FIELDS.map(({ field, label, unit }) => {
        const id = `${baseId}-${field}`;
        const invalid = view !== undefined && draft !== undefined && wrong?.field === field;
        const tokens = field === 'token_budget';
        return (
          <div className="agent-team-settings-field" key={field}>
            <label htmlFor={id}>{label}</label>
            <span className="agent-team-settings-input">
              <input
                aria-describedby={invalid ? `${baseId}-error` : undefined}
                aria-invalid={invalid || undefined}
                disabled={disabled || (tokens && draft?.unlimited === true)}
                id={id}
                inputMode="numeric"
                max={view === undefined ? undefined : (limitOf(view.maximum, field) ?? undefined)}
                min={view === undefined ? undefined : (limitOf(view.minimum, field) ?? undefined)}
                onChange={(event) => {
                  if (draft) onDraft({ ...draft, [field]: event.target.value });
                }}
                step={1}
                type="number"
                value={draft?.[field] ?? ''}
              />
              <span aria-hidden="true">{unit}</span>
              {tokens ? (
                <label className="agent-team-settings-unlimited">
                  <input
                    checked={draft?.unlimited ?? false}
                    disabled={disabled}
                    onChange={(event) => {
                      if (draft) onDraft({ ...draft, unlimited: event.target.checked });
                    }}
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
        <p
          className="settings-status-line"
          id={`${baseId}-error`}
          role="alert"
        >{`${wrong.label}：请输入 ${limitOf(view.minimum, wrong.field)} 到 ${limitOf(view.maximum, wrong.field)} 的整数。`}</p>
      ) : null}
      <div className="settings-card-actions">
        <button
          className="button is-primary is-small"
          disabled={
            disabled || view === undefined || chosen === undefined || same(chosen, view.limits)
          }
          type="submit"
        >
          保存
        </button>
        {view !== undefined && !same(view.limits, view.defaults) ? (
          <button className="button is-small" disabled={disabled} onClick={onDefault} type="button">
            恢复默认
          </button>
        ) : null}
      </div>
    </form>
  );
}
