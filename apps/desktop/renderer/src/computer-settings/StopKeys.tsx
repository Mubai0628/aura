import { useId, useState } from 'react';

export function StopKeys({
  value,
  disabled,
  save,
}: {
  value: string;
  disabled: boolean;
  save: (keys: string) => Promise<boolean>;
}) {
  const id = useId();
  const [draft, setDraft] = useState(value);
  return (
    <form
      className="computer-stop-keys"
      onSubmit={(event) => {
        event.preventDefault();
        if (!disabled && draft.trim()) void save(draft.trim());
      }}
    >
      <label htmlFor={id}>停止快捷键</label>
      <input
        id={id}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        disabled={disabled}
        spellCheck={false}
        autoComplete="off"
        maxLength={80}
      />
      <button
        className="button is-small"
        type="submit"
        disabled={disabled || !draft.trim() || draft.trim() === value}
      >
        保存快捷键
      </button>
    </form>
  );
}
