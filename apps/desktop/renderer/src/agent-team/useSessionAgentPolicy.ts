import { useEffect, useRef, useState } from 'react';
import type { AgentSessionPolicy } from '@aura/contracts/generated';
import { isAgentParams } from '../../../shared/agent-validation';

export function useSessionAgentPolicy(sessionId: string, demoMode: boolean) {
  const api = window.aura?.v1;
  const supported =
    !demoMode &&
    typeof api?.readSessionAgentPolicy === 'function' &&
    typeof api?.setSessionAgentPolicy === 'function' &&
    isAgentParams('agent.team.policy.get', { session_id: sessionId });
  const [policy, setPolicy] = useState<AgentSessionPolicy>();
  const [problem, setProblem] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [reading, setReading] = useState(true);
  const [uncertain, setUncertain] = useState(false);
  const [retry, setRetry] = useState(0);
  const active = useRef(true);
  useEffect(() => {
    active.current = true;
    return () => {
      active.current = false;
    };
  }, []);
  useEffect(() => {
    if (!supported || !api?.readSessionAgentPolicy) return;
    let alive = true;
    void (async () => {
      try {
        const result = await api.readSessionAgentPolicy({ session_id: sessionId });
        if (!alive) return;
        if (!result.ok || result.value.session_id !== sessionId) {
          setProblem('派生策略暂不可读，聊天不受影响。');
          return;
        }
        setPolicy(result.value);
        setProblem(undefined);
        setUncertain(false);
      } catch {
        if (alive) setProblem('派生策略暂不可读，聊天不受影响。');
      } finally {
        if (alive) setReading(false);
      }
    })();
    return () => {
      alive = false;
    };
  }, [api, sessionId, supported, retry]);
  const save = async (allowed: boolean) => {
    if (!api?.setSessionAgentPolicy || !policy || busy || reading || uncertain) return;
    setBusy(true);
    try {
      const result = await api.setSessionAgentPolicy({
        session_id: sessionId,
        expected_revision: policy.revision,
        allow_proactive_temporary_agents: allowed,
      });
      if (!active.current) return;
      if (!result.ok || result.value.session_id !== sessionId) {
        setProblem('策略未更新或已在别处变化，请重新读取后核对。');
        setUncertain(true);
        return;
      }
      setPolicy(result.value);
      setProblem(undefined);
    } catch {
      if (active.current) {
        setProblem('修改结果尚未确认，请重新读取，不要重复操作；聊天不受影响。');
        setUncertain(true);
      }
    } finally {
      if (active.current) setBusy(false);
    }
  };
  return {
    supported,
    policy,
    problem,
    busy,
    reading,
    uncertain,
    save,
    reload: () => {
      setReading(true);
      setRetry((value) => value + 1);
    },
  };
}
