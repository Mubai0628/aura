import { useState } from 'react';
import type { MemoryMaintenanceRun } from '@aura/contracts/generated';
import { MaintenanceLogDialog } from './MaintenanceLogDialog';
import { runSummary, runTime } from './maintenance-run-text';

/**
 * The project's maintenance runs: the newest three here, all of them in a dialog, and
 * the plain text log Core keeps opened in the user's editor.
 */
export function MaintenanceRuns({
  runs,
  logAvailable,
  busy,
  onOpenLog,
}: {
  runs: readonly MemoryMaintenanceRun[];
  logAvailable: boolean;
  busy: boolean;
  onOpenLog: () => Promise<{ completed: boolean; message: string } | undefined>;
}) {
  const [open, setOpen] = useState(false);
  const [notice, setNotice] = useState<{ completed: boolean; message: string }>();
  return (
    <section className="memory-runs settings-card" aria-label="整理记录">
      <header>
        <h3>整理记录</h3>
        <div className="settings-card-actions">
          <button
            type="button"
            className="button is-small"
            disabled={!runs.length}
            onClick={() => setOpen(true)}
          >
            查看全部
          </button>
          <button
            type="button"
            className="button is-small"
            disabled={!logAvailable || busy}
            onClick={() => void onOpenLog().then(setNotice)}
          >
            打开日志文件
          </button>
        </div>
      </header>
      {notice ? (
        <p
          className={notice.completed ? 'settings-status-line is-done' : 'settings-status-line'}
          role={notice.completed ? 'status' : 'alert'}
        >
          {notice.message}
        </p>
      ) : null}
      {runs.length ? (
        <ul className="memory-run-preview">
          {runs.slice(0, 3).map((run) => (
            <li key={run.run_id}>
              <time dateTime={run.finished_at ?? run.started_at}>{runTime(run)}</time>
              <span>{runSummary(run)}</span>
            </li>
          ))}
        </ul>
      ) : (
        <p>当前项目暂无整理记录。</p>
      )}
      {open ? <MaintenanceLogDialog runs={runs} onClose={() => setOpen(false)} /> : null}
    </section>
  );
}
