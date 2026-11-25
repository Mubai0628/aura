import type {
  AgentMainCommand,
  AgentMainParams,
  AgentMainResult,
} from '../shared/generated/routes.js';
import type { ResultEnvelope } from '../shared/aura-api.js';

export function agentProfileBridge(
  invoke: <C extends AgentMainCommand>(
    command: C,
    params: AgentMainParams[C],
  ) => Promise<ResultEnvelope<AgentMainResult[C]>>,
) {
  return {
    readAgentGraph: (request: AgentMainParams['agent.team.graph.get']) =>
      invoke('agent.team.graph.get', request),
    mutateAgentGraph: (request: AgentMainParams['agent.team.graph.mutate']) =>
      invoke('agent.team.graph.mutate', request),
    readPeerMailbox: (request: AgentMainParams['agent.team.peers.get']) =>
      invoke('agent.team.peers.get', request),
    managePeerMailbox: (request: AgentMainParams['agent.team.peers.manage']) =>
      invoke('agent.team.peers.manage', request),
    readSessionAgentPolicy: (request: AgentMainParams['agent.team.policy.get']) =>
      invoke('agent.team.policy.get', request),
    setSessionAgentPolicy: (request: AgentMainParams['agent.team.policy.set']) =>
      invoke('agent.team.policy.set', request),
    readAgentProfile: (request: AgentMainParams['agent.team.profile.get']) =>
      invoke('agent.team.profile.get', request),
    setAgentProfile: (request: AgentMainParams['agent.team.profile.set']) =>
      invoke('agent.team.profile.set', request),
    previewAgentProfile: (request: AgentMainParams['agent.team.profile.preview']) =>
      invoke('agent.team.profile.preview', request),
  };
}
