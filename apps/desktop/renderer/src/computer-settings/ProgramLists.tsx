import { useId, useState, type FormEvent } from 'react';
import type { ComputerAddedApp, ComputerApp } from '@aura/contracts/generated';

import { AppIcon } from '../app-icons';

/** The programs every conversation operates without asking, each one removable. */
export function AllowedPrograms({
  apps,
  disabled,
  onChange,
}: {
  apps: readonly ComputerApp[];
  disabled: boolean;
  onChange: (apps: ComputerApp[]) => Promise<boolean>;
}) {
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} className="computer-program-list">
      <h4 id={headingId}>总是允许的程序</h4>
      {apps.length > 0 ? (
        <ul>
          {apps.map((app) => (
            <li key={app.app_id} title={app.app_id}>
              <span>{app.name}</span>
              <button
                aria-label={`移除 ${app.name}`}
                className="icon-button"
                disabled={disabled}
                onClick={() => void onChange(apps.filter((item) => item.app_id !== app.app_id))}
                type="button"
              >
                <AppIcon name="close" size={13} />
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="computer-program-none">无</p>
      )}
    </section>
  );
}

/** The last part of a path, the way the list names a program added without a name. */
const fileName = (path: string) =>
  path.slice(Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/')) + 1);

/** Programs the Start menu does not list, added by their full path. */
export function AddedPrograms({
  apps,
  disabled,
  onChange,
}: {
  apps: readonly ComputerAddedApp[];
  disabled: boolean;
  onChange: (apps: ComputerAddedApp[]) => Promise<boolean>;
}) {
  const [draft, setDraft] = useState('');
  const headingId = useId();
  const inputId = useId();
  const add = (event: FormEvent) => {
    event.preventDefault();
    const path = draft.trim().replace(/^"(.*)"$/u, '$1');
    if (path === '') return;
    void onChange([...apps, { path }]).then((saved) => {
      if (saved) setDraft('');
    });
  };
  return (
    <section aria-labelledby={headingId} className="computer-program-list">
      <h4 id={headingId}>手动添加的程序</h4>
      {apps.length > 0 ? (
        <ul>
          {apps.map((app) => (
            <li key={app.path} title={app.path}>
              <span>{app.name ?? fileName(app.path)}</span>
              <button
                aria-label={`移除 ${app.name ?? fileName(app.path)}`}
                className="icon-button"
                disabled={disabled}
                onClick={() => void onChange(apps.filter((item) => item.path !== app.path))}
                type="button"
              >
                <AppIcon name="close" size={13} />
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      <form onSubmit={add}>
        <label htmlFor={inputId}>程序路径</label>
        <input
          autoComplete="off"
          disabled={disabled}
          id={inputId}
          onChange={(event) => setDraft(event.target.value)}
          spellCheck={false}
          value={draft}
        />
        <button
          className="button is-small"
          disabled={disabled || draft.trim() === ''}
          type="submit"
        >
          添加
        </button>
      </form>
    </section>
  );
}
