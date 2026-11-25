import { useId, useState } from 'react';
import { AppIcon } from '../../app-icons';

export function PeerBudgetForm({
  min,
  initial,
  label,
  busy,
  onSave,
}: {
  min: number;
  initial: number;
  label: string;
  busy: boolean;
  onSave: (limit: number) => void;
}) {
  const id = useId();
  const [text, setText] = useState(String(initial));
  const limit = Number(text);
  const invalid = !text.trim() || !Number.isInteger(limit) || limit < min || limit > 1024;
  return (
    <form
      className="peer-budget-form"
      noValidate
      onSubmit={(event) => {
        event.preventDefault();
        if (!busy && !invalid) onSave(limit);
      }}
    >
      <label htmlFor={id}>{label}</label>
      <input
        aria-describedby={invalid ? `${id}-error` : undefined}
        aria-invalid={invalid || undefined}
        id={id}
        type="number"
        min={min}
        max={1024}
        step={1}
        value={text}
        onChange={(event) => setText(event.target.value)}
      />
      <button className="button is-small" type="submit" disabled={busy || invalid}>
        <AppIcon name="check" />
        保存额度
      </button>
      {invalid ? (
        <span id={`${id}-error`} role="alert">
          请输入 {min} 到 1024 的整数。
        </span>
      ) : null}
    </form>
  );
}
