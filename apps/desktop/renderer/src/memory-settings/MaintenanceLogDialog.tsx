import { useState } from 'react';
import type { MemoryMaintenanceRun } from '@aura/contracts/generated';
import { AppIcon } from '../app-icons';
import { ModalDialog } from '../ModalDialog';
import { runDetail, runSummary, runTime } from './maintenance-run-text';

const PAGE = 20;

/** Every run the page holds, newest first, a page at a time. */
export function MaintenanceLogDialog({
  runs,
  onClose,
}: {
  runs: readonly MemoryMaintenanceRun[];
  onClose: () => void;
}) {
  const [shown, setShown] = useState(PAGE);
  const rest = runs.length - shown;
  return (
    <ModalDialog
      className="maintenance-log-dialog"
      onClose={onClose}
      titleId="maintenance-log-title"
    >
      <header className="modal-header">
        <h2 id="maintenance-log-title">整理日志</h2>
        <button aria-label="关闭整理日志" className="icon-button" onClick={onClose} type="button">
          <AppIcon name="close" />
        </button>
      </header>
      <div className="maintenance-log-body">
        <ol className="maintenance-log-list">
          {runs.slice(0, shown).map((run) => (
            <li key={run.run_id} data-state={run.state}>
              <time dateTime={run.finished_at ?? run.started_at}>{runTime(run)}</time>
              <strong>{runSummary(run)}</strong>
              <small>{runDetail(run)}</small>
            </li>
          ))}
        </ol>
        {rest > 0 ? (
          <button type="button" className="button is-small" onClick={() => setShown(shown + PAGE)}>
            显示更早的 {Math.min(rest, PAGE)} 条
          </button>
        ) : null}
      </div>
    </ModalDialog>
  );
}
