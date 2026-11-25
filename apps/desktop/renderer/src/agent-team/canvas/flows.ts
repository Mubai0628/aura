import type { AgentTeamMemberView, AgentTeamView } from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';

/** What passes along a line: work for the sub-agent, or its report of one kind. */
export type FlowKind = 'task' | 'report' | 'question' | 'failed';

export interface Flow {
  kind: FlowKind;
  /** From the sub-agent to the main agent. */
  up: boolean;
  label: string;
}

/** How long a line shows what passed along it. */
export const FLOW_MS = 2400;

export function teamLineFlow(
  member: AgentTeamMemberView,
  recent: Flow | undefined,
): Pick<Flow, 'kind' | 'up'> | undefined {
  if (member.state === 'running' && member.working) return { kind: 'task', up: false };
  if (recent?.up || member.state === 'running') return recent;
  return undefined;
}

function reported(member: AgentTeamMemberView): Flow {
  switch (member.last_report) {
    case 'waiting':
      return { kind: 'question', up: true, label: '提问' };
    case 'blocked':
      return { kind: 'question', up: true, label: '受阻' };
    case 'failed':
      return { kind: 'failed', up: true, label: '没做成' };
    case 'progress':
      return { kind: 'report', up: true, label: '进展' };
    default:
      return { kind: 'report', up: true, label: '报告' };
  }
}

/**
 * What passed between the main agent and each sub-agent from one list of them to the next: a new
 * sub-agent got its task sheet, a new report went up (for one just stopped, the report Aura gives
 * the main agent about the stop), and one that had reported took more work.
 */
export function exchanges(
  before: AgentTeamView | undefined,
  after: AgentTeamView,
): Map<string, Flow> {
  const flows = new Map<string, Flow>();
  if (!before) return flows;
  const was = new Map(before.members.map((member) => [member.agent, member]));
  for (const member of after.members) {
    const old = was.get(member.agent);
    if (!old) flows.set(member.agent, { kind: 'task', up: false, label: '任务单' });
    else if (
      member.last_report &&
      (member.last_report !== old.last_report || member.summary !== old.summary)
    )
      flows.set(
        member.agent,
        member.state === 'stopped' && old.state !== 'stopped'
          ? { kind: 'failed', up: true, label: '已停止' }
          : reported(member),
      );
    else if (old.state !== 'running' && member.state === 'running')
      flows.set(member.agent, { kind: 'task', up: false, label: '消息' });
  }
  return flows;
}

/** The lines showing what just passed along them, each for [`FLOW_MS`]. */
export function useTeamFlows(view: AgentTeamView | undefined): ReadonlyMap<string, Flow> {
  const previous = useRef<AgentTeamView | undefined>(undefined);
  const timers = useRef(new Map<string, number>());
  const [flows, setFlows] = useState<ReadonlyMap<string, Flow>>(() => new Map());
  useEffect(() => {
    if (!view) return;
    const fresh = exchanges(previous.current, view);
    previous.current = view;
    if (!fresh.size) return;
    setFlows((current) => new Map([...current, ...fresh]));
    for (const [agent, flow] of fresh) {
      window.clearTimeout(timers.current.get(agent));
      timers.current.set(
        agent,
        window.setTimeout(() => {
          timers.current.delete(agent);
          setFlows((current) => {
            if (current.get(agent) !== flow) return current;
            const next = new Map(current);
            next.delete(agent);
            return next;
          });
        }, FLOW_MS),
      );
    }
  }, [view]);
  useEffect(() => {
    const running = timers.current;
    return () => {
      for (const timer of running.values()) window.clearTimeout(timer);
      running.clear();
    };
  }, []);
  return flows;
}
