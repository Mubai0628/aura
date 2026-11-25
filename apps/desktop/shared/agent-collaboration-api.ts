import type { AgentMainParams, AgentMainResult } from './generated/routes.js';
import type { ResultEnvelope } from './aura-api.js';

export interface AgentCollaborationApi {
  readAgentGraph(
    request: AgentMainParams['agent.team.graph.get'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.team.graph.get']>>;
  mutateAgentGraph(
    request: AgentMainParams['agent.team.graph.mutate'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.team.graph.mutate']>>;
  readPeerMailbox(
    request: AgentMainParams['agent.team.peers.get'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.team.peers.get']>>;
  managePeerMailbox(
    request: AgentMainParams['agent.team.peers.manage'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.team.peers.manage']>>;
  readSessionAgentPolicy(
    request: AgentMainParams['agent.team.policy.get'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.team.policy.get']>>;
  setSessionAgentPolicy(
    request: AgentMainParams['agent.team.policy.set'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.team.policy.set']>>;
  readAgentProfile(
    request: AgentMainParams['agent.team.profile.get'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.team.profile.get']>>;
  setAgentProfile(
    request: AgentMainParams['agent.team.profile.set'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.team.profile.set']>>;
  previewAgentProfile(
    request: AgentMainParams['agent.team.profile.preview'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.team.profile.preview']>>;
  listAgentTeam(
    request: AgentMainParams['agent.team.list'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.team.list']>>;
  stopAgentTeamMember(
    request: AgentMainParams['agent.team.stop'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.team.stop']>>;
}
