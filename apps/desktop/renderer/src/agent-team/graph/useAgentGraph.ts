import type {
  AgentGraphAction,
  AgentGraphRevision,
  AgentGraphView,
} from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';

export type GraphChange = (
  action: AgentGraphAction,
  expected?: AgentGraphRevision,
) => Promise<boolean>;
interface State {
  session: string;
  view?: AgentGraphView;
  busy: boolean;
  uncertain: boolean;
  problem?: string;
}
interface Scope {
  session: string;
  alive: boolean;
  pending: boolean;
  uncertain: boolean;
  reading: boolean;
  version: number;
  view?: AgentGraphView;
}
export const graphRevision = (view: AgentGraphView): AgentGraphRevision => ({
  graph_id: view.graph.graph_id ?? null,
  revision: view.graph.revision,
});

export function useAgentGraph(sessionId: string, visible: boolean) {
  const api = window.aura?.v1;
  const [state, setState] = useState<State>({ session: sessionId, busy: false, uncertain: true });
  const [retry, setRetry] = useState(0);
  const scope = useRef<Scope | undefined>(undefined);
  useEffect(() => {
    if (!visible || !api?.readAgentGraph) return;
    const current: Scope = {
      session: sessionId,
      alive: true,
      pending: false,
      uncertain: false,
      reading: false,
      version: 0,
    };
    scope.current = current;
    const read = async () => {
      if (current.reading || current.pending || current.uncertain) return;
      current.reading = true;
      const version = current.version;
      try {
        const result = await api.readAgentGraph({ session_id: sessionId });
        if (!current.alive || current.version !== version) return;
        if (!result.ok || !result.value.graph.sessions.includes(sessionId))
          throw new Error('graph_read');
        const next = result.value;
        if (
          current.view &&
          current.view.graph.graph_id === next.graph.graph_id &&
          current.view.graph.revision > next.graph.revision
        )
          return;
        current.view = next;
        setState({ session: sessionId, view: next, busy: false, uncertain: false });
      } catch {
        if (current.alive && current.version === version) {
          current.uncertain = true;
          setState((old) => ({
            session: sessionId,
            ...(old.session === sessionId && old.view ? { view: old.view } : {}),
            busy: false,
            uncertain: true,
            problem: '编排画布暂时读不到，暂不修改；原临时团队仍可查看，请重新读取。',
          }));
        }
      } finally {
        current.reading = false;
      }
    };
    void read();
    const timer = window.setInterval(() => void read(), 3000);
    return () => {
      current.alive = false;
      window.clearInterval(timer);
    };
  }, [api, sessionId, visible, retry]);

  const change: GraphChange = async (action, expected) => {
    const current = scope.current;
    if (
      !current?.alive ||
      current.session !== sessionId ||
      !current.view ||
      current.pending ||
      current.uncertain ||
      !api?.mutateAgentGraph
    )
      return false;
    current.pending = true;
    current.version += 1;
    setState((old) => ({ ...old, busy: true }));
    try {
      const result = await api.mutateAgentGraph({
        session_id: sessionId,
        expected_graph: expected ?? graphRevision(current.view),
        action,
      });
      if (!current.alive || scope.current !== current) return false;
      if (!result.ok || !result.value.graph.sessions.includes(sessionId))
        throw new Error('graph_write');
      current.view = result.value;
      setState({ session: sessionId, view: result.value, busy: false, uncertain: false });
      return true;
    } catch {
      if (current.alive && scope.current === current) {
        current.uncertain = true;
        setState((old) => ({
          ...old,
          busy: false,
          uncertain: true,
          problem: '修改结果尚未确认，或画布已经变化；不会自动重复操作，请重新读取后核对。',
        }));
      }
      return false;
    } finally {
      current.pending = false;
    }
  };
  const current = state.session === sessionId;
  return {
    view: current ? state.view : undefined,
    busy: visible && current && state.busy,
    uncertain: !visible || !current || state.uncertain,
    problem: current ? state.problem : undefined,
    change,
    refresh: () => {
      if (scope.current?.pending) return;
      setState((old) => ({
        session: sessionId,
        ...(old.session === sessionId && old.view ? { view: old.view } : {}),
        busy: false,
        uncertain: true,
      }));
      setRetry((n) => n + 1);
    },
  };
}
