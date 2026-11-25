import type { AgentGraphView, AgentProfile, AgentTeamView } from '@aura/contracts/generated';

export const profile = (session_id: string, name: string): AgentProfile => ({
  session_id,
  workspace_id: 'wsp_home',
  name,
  description: '',
  description_source: 'none',
  revision: 0,
  input_tokens: 0,
  output_tokens: 0,
  usage_complete: true,
});
export const team = (parent: string): AgentTeamView => ({
  lead: { model_profile_id: 'mdl_fixture', model: 'Fixture', working: false, tokens: 0 },
  members: [
    {
      agent: 'A1',
      session_id: `${parent}_child`,
      name: '检查',
      model_profile_id: 'mdl_fixture',
      model: 'Fixture',
      state: 'reported',
      working: false,
      approvals: [],
      tokens: 40,
      mode: 'read_only',
      workspace_id: 'wsp_home',
      allowed: { deletions: false, downloads: false, outside_writes: false },
      last_report: 'ready',
      summary: '旧报告仍在',
    },
  ],
});
export function graphView(
  roots: readonly [string, string][],
  id = 'acg_one',
  revision = 7,
): AgentGraphView {
  return {
    graph: {
      graph_id: id,
      workspace_id: 'wsp_home',
      revision,
      sessions: roots.map(([session]) => session),
      edges: [],
      positions: [],
    },
    profiles: roots.map(([session, name]) => profile(session, name)),
    teams: roots.map(([parent_session_id]) => ({
      parent_session_id,
      team: team(parent_session_id),
    })),
    candidates: [],
  };
}
