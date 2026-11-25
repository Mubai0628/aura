import { ToggleSwitch } from '../ui/ToggleSwitch';
import { useSessionAgentPolicy } from './useSessionAgentPolicy';
import './session-agent-policy.css';

type Props = { sessionId: string; demoMode?: boolean };
export function SessionAgentPolicyControl({ sessionId, demoMode = false }: Props) {
  return <BoundPolicy key={sessionId} sessionId={sessionId} demoMode={demoMode} />;
}

export function AllowSubagentsToggle({
  checked,
  disabled,
  onChange,
}: {
  checked?: boolean | undefined;
  disabled: boolean;
  onChange: (allowed: boolean) => void;
}) {
  return (
    <label className="session-agent-policy-toggle">
      <ToggleSwitch
        checked={checked}
        disabled={disabled}
        label="允许子 Agent"
        onCheckedChange={onChange}
      />
      <span aria-hidden="true">允许子 Agent</span>
    </label>
  );
}

function BoundPolicy({ sessionId, demoMode = false }: Props) {
  const state = useSessionAgentPolicy(sessionId, demoMode);
  if (!state.supported) return null;
  return (
    <div className="session-agent-policy">
      <AllowSubagentsToggle
        checked={state.policy?.allow_proactive_temporary_agents}
        disabled={state.busy || state.reading || state.uncertain || !state.policy}
        onChange={(allowed) => void state.save(allowed)}
      />
      {state.problem ? (
        <span className="session-agent-policy-status" role="status">
          {state.problem}
          <button disabled={state.busy || state.reading} onClick={state.reload} type="button">
            重新读取策略
          </button>
        </span>
      ) : null}
    </div>
  );
}
