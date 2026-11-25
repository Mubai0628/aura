import { ToggleSwitch } from '../ui/ToggleSwitch';
import { PeerBudgetForm } from '../agent-team/peer/PeerBudgetForm';
import { usePeerInbox } from '../agent-team/peer/usePeerInbox';

export function PeerCollaborationSettings({ coreReady }: { coreReady: boolean }) {
  const settings = usePeerInbox(null, coreReady, 'settings');
  if (!settings.supported) return null;
  const budget = settings.value?.budget_default;
  const policy = settings.value?.review_policy;
  return (
    <section className="settings-surface agent-team-settings-peer" aria-label="同级协作">
      <h3>同级协作</h3>
      {budget ? (
        <PeerBudgetForm
          key={budget.revision}
          min={1}
          initial={budget.limit}
          label="新协作默认额度"
          busy={!coreReady || settings.busy}
          onSave={(limit) =>
            void settings.change({ kind: 'set_default', expected_revision: budget.revision, limit })
          }
        />
      ) : null}
      <label className="agent-team-settings-peer-policy">
        <ToggleSwitch
          label="新投递先由我确认"
          checked={policy?.required}
          disabled={!coreReady || !policy || settings.busy}
          onCheckedChange={(required) => {
            if (policy)
              void settings.change({
                kind: 'set_review',
                expected_revision: policy.revision,
                required,
              });
          }}
        />
        <span aria-hidden="true">新投递先由我确认</span>
      </label>
      <SettingsProblem error={settings.error} onRetry={() => void settings.refresh()} />
    </section>
  );
}

function SettingsProblem({ error, onRetry }: { error: string; onRetry: () => void }) {
  return error ? (
    <p className="settings-status-line" role="alert">
      {error}
      <button className="button is-small" type="button" onClick={onRetry}>
        重新读取
      </button>
    </p>
  ) : null;
}
