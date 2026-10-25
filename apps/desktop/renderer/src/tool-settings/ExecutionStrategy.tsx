import { useState } from 'react';
import type { AgentToolSettingsView } from '@aura/contracts/generated';
import { userMessage } from '../agent-session/user-message';
import type { ConfigurationOwner } from './useAgentConfiguration';
import './execution-strategy.css';

/**
 * The execution-strategy editor: dynamic schema cap, ordinary-tool global
 * concurrency, default timeout and maximum retries.
 *
 * Rust owns the ranges and the optimistic-concurrency revision; this component
 * only carries the values back. The committed snapshot and its revision come from
 * `useAgentConfiguration`, so saving the tool list and then the strategy cannot
 * submit a stale revision. A rejected save keeps what the user typed, so a range
 * error or a revision conflict never silently discards the edit.
 */
type FieldName =
  'max_dynamic_tool_schemas' | 'max_concurrent_tool_calls' | 'default_timeout_ms' | 'max_retries';

const FIELDS: {
  name: FieldName;
  label: string;
  min: number;
  max: number;
}[] = [
  {
    name: 'max_dynamic_tool_schemas',
    label: '动态 Schema 上限',
    min: 1,
    max: 4294967295,
  },
  {
    name: 'max_concurrent_tool_calls',
    label: '普通工具全局并发',
    min: 1,
    max: 32,
  },
  {
    name: 'default_timeout_ms',
    label: '默认 Timeout（毫秒）',
    min: 1_000,
    max: 1_800_000,
  },
  {
    name: 'max_retries',
    label: '最大重试次数',
    min: 0,
    max: 3,
  },
];

export function ExecutionStrategy({ configuration }: { configuration: ConfigurationOwner }) {
  const { view, loadError, busy, reload, commit } = configuration;
  const [draft, setDraft] = useState<Record<FieldName, string>>();
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string>();
  const [failedField, setFailedField] = useState<FieldName>();

  const fromView = (next: AgentToolSettingsView): Record<FieldName, string> => ({
    max_dynamic_tool_schemas: String(next.max_dynamic_tool_schemas),
    max_concurrent_tool_calls: String(next.max_concurrent_tool_calls),
    default_timeout_ms: String(next.default_timeout_ms),
    max_retries: String(next.max_retries),
  });

  // The committed snapshot belongs to `useAgentConfiguration`, so this editor never
  // holds a revision of its own: a save made in the tool list cannot leave this
  // editor submitting a stale revision. `undefined` means "nothing unsaved here", so
  // the fields show the shared snapshot itself. A snapshot that arrives while the
  // user has typed something is therefore ignored rather than copied over the
  // draft, and this editor's own successful save is what drops the draft.
  const committed = view === undefined ? undefined : fromView(view);
  const value = draft ?? committed;

  const bounds = (name: FieldName) => FIELDS.find((field) => field.name === name);
  const fieldError = (name: FieldName) => {
    if (failedField === name) return true;
    const raw = value?.[name];
    if (raw === undefined) return false;
    const field = bounds(name);
    if (field === undefined || !Number.isInteger(Number(raw))) return true;
    return Number(raw) < field.min || Number(raw) > field.max;
  };
  const changed =
    committed !== undefined &&
    draft !== undefined &&
    FIELDS.some((field) => draft[field.name] !== committed[field.name]);
  const valid = value !== undefined && FIELDS.every((field) => !fieldError(field.name));

  const save = async () => {
    if (view === undefined || value === undefined || busy || !valid) return;
    setSaved(false);
    setError(undefined);
    const outcome = await commit({
      max_dynamic_tool_schemas: Number(value.max_dynamic_tool_schemas),
      max_concurrent_tool_calls: Number(value.max_concurrent_tool_calls),
      default_timeout_ms: Number(value.default_timeout_ms),
      max_retries: Number(value.max_retries),
    });
    if (outcome.error !== undefined) {
      // Keep the user's input. A field-level rejection names the field; a
      // revision conflict asks for a reload rather than overwriting.
      setError(outcome.error);
      const named = FIELDS.find((field) => outcome.error?.includes(field.name));
      setFailedField(named?.name);
    } else {
      setSaved(true);
      // This editor's own successful save is what replaces its draft; the shared
      // owner has already adopted the values Core committed.
      setDraft(undefined);
      setFailedField(undefined);
    }
  };

  if (loadError !== undefined && view === undefined) {
    return (
      <div className="execution-strategy">
        <p role="alert">{userMessage(loadError)}</p>
        <button type="button" onClick={() => void reload()}>
          重新读取执行策略
        </button>
      </div>
    );
  }
  if (view === undefined || value === undefined) {
    return <p className="execution-strategy-pending">正在读取执行策略…</p>;
  }

  return (
    <div className="execution-strategy">
      <div className="execution-strategy-fields">
        {FIELDS.map((field) => (
          <label key={field.name} data-invalid={fieldError(field.name) ? 'true' : undefined}>
            <span className="execution-strategy-label">{field.label}</span>
            <input
              type="number"
              min={field.min}
              max={field.max}
              step="1"
              value={value[field.name]}
              disabled={busy}
              aria-label={field.label}
              aria-invalid={fieldError(field.name) ? true : undefined}
              onChange={(event) => {
                const next = event.target.value;
                setDraft({ ...value, [field.name]: next });
                setSaved(false);
                setFailedField(undefined);
              }}
            />
          </label>
        ))}
      </div>
      <div className="execution-strategy-actions">
        <button
          type="button"
          className="button is-primary"
          disabled={!changed || busy || !valid}
          onClick={() => void save()}
        >
          {busy ? '保存中…' : '保存执行策略'}
        </button>
        <span className="execution-strategy-revision">策略修订 {view.revision}</span>
      </div>
      {error ? <p role="alert">{userMessage(error)}</p> : null}
      {saved ? <p role="status">执行策略已保存。</p> : null}
    </div>
  );
}
