import { useState } from 'react';
import type { MemorySettingsSnapshot, WorkspaceId } from '@aura/contracts/generated';
import { AppIcon } from '../app-icons';
import { useMemory } from './useMemory';
import { VaultPanel } from './VaultPanel';
import { MaintenanceRuns } from './MaintenanceRuns';
import { ToggleSwitch } from '../ui/ToggleSwitch';
import { SkeletonRows } from '../ui/SkeletonRows';
import './memory-settings.css';
export function MemorySettings({
  coreReady,
  workspaceId,
}: {
  coreReady: boolean;
  workspaceId?: WorkspaceId | undefined;
}) {
  const state = useMemory(coreReady, workspaceId);
  return (
    <section className="memory-settings settings-page" aria-label="记忆设置">
      <header className="settings-content-heading">
        <div>
          <h2>记忆</h2>
        </div>
        <div className="settings-page-actions">
          <button
            type="button"
            className="button"
            disabled={state.busy}
            aria-busy={state.busy ? true : undefined}
            onClick={() => void state.reload()}
          >
            <AppIcon name="refresh" size={15} />
            刷新记忆状态
          </button>
        </div>
      </header>
      {state.error ? (
        <p className="settings-status-line" role="alert">
          {state.error}
        </p>
      ) : null}
      {state.message ? (
        <p className="settings-status-line is-done" role="status">
          {state.message}
        </p>
      ) : null}
      {!coreReady ? (
        <p className="settings-status-line" role="status">
          Core 尚未就绪，记忆管理暂不可用。
        </p>
      ) : !state.snapshot && !state.error ? (
        <SkeletonRows label="正在读取记忆" rows={2} />
      ) : null}
      {state.settings ? (
        <MaintenanceForm
          key={`${state.settings.revision}-${state.settings.issue}`}
          snapshot={state.settings}
          busy={state.busy}
          save={async (settings) => {
            const result = await state.act(
              () =>
                state.call('memory.settings.update', {
                  expected_revision: state.settings!.revision!,
                  settings,
                }),
              '记忆设置已保存',
            );
            if (result) await state.reload();
          }}
        />
      ) : null}
      {state.snapshot?.vaults.map((vault) => (
        <VaultPanel key={vault.scope} vault={vault} workspaceId={workspaceId} state={state} />
      ))}
      {!workspaceId ? <p className="memory-hint">选择工作区后可查看该项目的记忆。</p> : null}
      <MaintenanceRuns
        runs={state.snapshot?.recent_runs ?? []}
        logAvailable={Boolean(
          state.snapshot?.vaults.find((vault) => vault.scope === 'project')?.maintenance_log,
        )}
        busy={state.busy}
        onOpenLog={() =>
          state.act(() =>
            state.call('memory.maintenance.open_log', {
              scope: 'project',
              workspace_id: workspaceId ?? null,
            }),
          )
        }
      />
    </section>
  );
}
function MaintenanceForm({
  snapshot,
  busy,
  save,
}: {
  snapshot: MemorySettingsSnapshot;
  busy: boolean;
  save: (settings: MemorySettingsSnapshot['settings']) => Promise<void>;
}) {
  const [enabled, setEnabled] = useState(snapshot.settings.automatic_maintenance);
  const [days, setDays] = useState(String(snapshot.settings.event_retention_days));
  const number = Number(days);
  const valid = Number.isInteger(number) && number >= 1 && number <= 3650;
  const changed =
    enabled !== snapshot.settings.automatic_maintenance ||
    number !== snapshot.settings.event_retention_days;
  return (
    <form
      className="memory-maintenance settings-card"
      aria-label="自动整理设置"
      onSubmit={(event) => {
        event.preventDefault();
        if (valid && snapshot.revision != null)
          void save({ automatic_maintenance: enabled, event_retention_days: number });
      }}
    >
      <h3>自动整理</h3>
      {snapshot.issue ? <p role="alert">{snapshot.issue}</p> : null}
      <label className="memory-switch">
        <ToggleSwitch
          checked={enabled}
          disabled={busy || snapshot.revision == null}
          label="任务完成后自动整理记忆"
          onCheckedChange={setEnabled}
        />
        任务完成后自动整理记忆
      </label>
      <label>
        事件保留天数
        <input
          type="number"
          min={1}
          max={3650}
          step={1}
          value={days}
          disabled={busy || snapshot.revision == null}
          aria-invalid={!valid}
          aria-describedby={!valid ? 'memory-retention-error' : undefined}
          onChange={(e) => setDays(e.target.value)}
        />
      </label>
      {!valid ? (
        <p id="memory-retention-error" role="alert">
          请输入 1 到 3650 的整数。
        </p>
      ) : null}
      <button
        type="submit"
        className="button is-primary"
        disabled={busy || !changed || !valid || snapshot.revision == null}
      >
        保存记忆设置
      </button>
    </form>
  );
}
