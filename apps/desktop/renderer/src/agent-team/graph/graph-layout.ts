import type { AgentGraphPosition, AgentGraphView } from '@aura/contracts/generated';
import {
  defaultLayout,
  LEAD_KEY,
  MEMBER,
  ROW_LENGTH,
  type Point,
  type Positions,
} from '../canvas/layout';

export function graphPositions(view: AgentGraphView): Record<string, Point> {
  const saved = new Map(view.graph.positions.map((position) => [position.session_id, position]));
  const teams = new Map(view.teams.map((entry) => [entry.parent_session_id, entry.team]));
  const rows = Math.max(
    1,
    ...view.teams.map((entry) => Math.ceil(entry.team.members.length / ROW_LENGTH)),
  );
  const columns = Math.max(
    1,
    ...view.teams.map((entry) => Math.min(ROW_LENGTH, entry.team.members.length)),
  );
  const columnStep = columns * MEMBER.width + (columns - 1) * 20 + 96;
  const rowHeight = 300 + rows * 144;
  const result: Record<string, Point> = {};
  view.graph.sessions.forEach((parent, index) => {
    const members = teams.get(parent)?.members ?? [];
    const defaults = defaultLayout(members.map((member) => member.session_id));
    const root = defaults[LEAD_KEY]!;
    const offset = { x: (index % 2) * columnStep, y: Math.floor(index / 2) * rowHeight };
    const anchor = saved.get(parent) ?? { x: root.x + offset.x, y: root.y + offset.y };
    result[parent] = { x: anchor.x, y: anchor.y };
    for (const member of members) {
      const at = saved.get(member.session_id) ?? {
        x: defaults[member.session_id]!.x + anchor.x - root.x,
        y: defaults[member.session_id]!.y + anchor.y - root.y,
      };
      result[member.session_id] = { x: at.x, y: at.y };
    }
  });
  return result;
}
export const positionsPayload = (positions: Positions): AgentGraphPosition[] =>
  Object.entries(positions).map(([session_id, at]) => ({
    session_id,
    x: Math.round(at.x),
    y: Math.round(at.y),
  }));

/** The destination stays still; the source's entire frame moves beside it, without scaling. */
export function mergePositions(
  destination: AgentGraphView,
  source: AgentGraphView,
): AgentGraphPosition[] {
  const one = graphPositions(destination);
  const two = graphPositions(source);
  const a = Object.values(one),
    b = Object.values(two);
  if (!a.length || !b.length) return positionsPayload({ ...one, ...two });
  const dx =
    Math.max(...a.map((at) => at.x + MEMBER.width)) + 96 - Math.min(...b.map((at) => at.x));
  const dy = Math.min(...a.map((at) => at.y)) - Math.min(...b.map((at) => at.y));
  return positionsPayload({
    ...one,
    ...Object.fromEntries(
      Object.entries(two).map(([id, at]) => [id, { x: at.x + dx, y: at.y + dy }]),
    ),
  });
}
/** Keep the stored graph intact when the workspace navigator hides archived sessions. */
export function visibleGraph(view: AgentGraphView, sessions: ReadonlySet<string>): AgentGraphView {
  const teams = view.teams.filter((entry) => sessions.has(entry.parent_session_id));
  const nodes = new Set(
    teams.flatMap((entry) => [
      entry.parent_session_id,
      ...entry.team.members.map((member) => member.session_id),
    ]),
  );
  return {
    ...view,
    graph: {
      ...view.graph,
      sessions: view.graph.sessions.filter((id) => sessions.has(id)),
      edges: view.graph.edges.filter(
        (edge) => sessions.has(edge.source_session_id) && sessions.has(edge.target_session_id),
      ),
      positions: view.graph.positions.filter((position) => nodes.has(position.session_id)),
    },
    profiles: view.profiles.filter((profile) => sessions.has(profile.session_id)),
    candidates: view.candidates.filter((profile) => sessions.has(profile.session_id)),
    teams,
  };
}
