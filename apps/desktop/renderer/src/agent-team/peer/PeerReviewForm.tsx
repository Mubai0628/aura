import { useId, useState } from 'react';

export function PeerReviewForm({
  original,
  busy,
  onRelease,
}: {
  original: string;
  busy: boolean;
  onRelease: (text: string | null) => void;
}) {
  const id = useId();
  const [revised, setRevised] = useState(original);
  const invalid = !revised.trim();
  return (
    <form
      noValidate
      onSubmit={(event) => {
        event.preventDefault();
        if (!busy && !invalid) onRelease(revised === original ? null : revised);
      }}
    >
      <label htmlFor={id}>接收内容</label>
      <textarea
        id={id}
        value={revised}
        maxLength={8000}
        aria-required="true"
        aria-invalid={invalid || undefined}
        aria-describedby={invalid ? `${id}-error` : undefined}
        onChange={(event) => setRevised(event.target.value)}
      />
      {invalid ? (
        <p id={`${id}-error`} role="alert">
          请输入接收内容。
        </p>
      ) : null}
      <button className="button is-primary is-small" type="submit" disabled={busy || invalid}>
        确认放行
      </button>
    </form>
  );
}
