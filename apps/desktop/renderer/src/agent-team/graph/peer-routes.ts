import type { AgentGraphEdge } from '@aura/contracts/generated';
import { LEAD, type Point, type Positions, type Route } from '../canvas/layout';

const ARROW_GAP = 12;
const TURN_CLEARANCE = 12;
const CARD_CLEARANCE = 4;
const SIDES = [
  { x: 1, y: 0 },
  { x: -1, y: 0 },
  { x: 0, y: 1 },
  { x: 0, y: -1 },
];

type Bounds = { left: number; right: number; top: number; bottom: number };
type Card = { id: string; at: Point; bounds: Bounds };
type Path = { corners: Route; segments: Bounds[]; length: number };
type Score = { cards: number; contacts: number; cost: number };

function bounds(a: Point, b: Point): Bounds {
  return {
    left: Math.min(a.x, b.x),
    right: Math.max(a.x, b.x),
    top: Math.min(a.y, b.y),
    bottom: Math.max(a.y, b.y),
  };
}

function touches(a: Bounds, b: Bounds) {
  return a.left <= b.right && a.right >= b.left && a.top <= b.bottom && a.bottom >= b.top;
}

function enters(segment: Bounds, card: Bounds, margin: number) {
  return (
    segment.left < card.right + margin &&
    segment.right > card.left - margin &&
    segment.top < card.bottom + margin &&
    segment.bottom > card.top - margin
  );
}

function contactCost(a: Bounds, b: Bounds): number {
  if (!touches(a, b)) return 0;
  const sharedLength =
    Math.min(a.right, b.right) > Math.max(a.left, b.left) ||
    Math.min(a.bottom, b.bottom) > Math.max(a.top, b.top);
  return sharedLength ? 2 : 1;
}

function path(corners: Route): Path | undefined {
  const points: Point[] = [];
  for (const corner of corners) {
    // Score the same integer coordinates used by the rendered SVG and hit area.
    const next = { x: Math.round(corner.x), y: Math.round(corner.y) };
    const last = points.at(-1);
    if (!last) {
      points.push(next);
      continue;
    }
    if (last.x !== next.x && last.y !== next.y) return;
    if (last.x === next.x && last.y === next.y) continue;
    const before = points.at(-2);
    if (
      before &&
      ((before.x === last.x && last.x === next.x) || (before.y === last.y && last.y === next.y))
    ) {
      const direction =
        (last.x - before.x) * (next.x - last.x) + (last.y - before.y) * (next.y - last.y);
      if (direction < 0) return;
      points[points.length - 1] = next;
    } else points.push(next);
  }
  const segments = points.slice(1).map((end, index) => bounds(points[index]!, end));
  if (
    !segments.length ||
    segments.some(
      (s, index) => index > 1 && segments.slice(0, index - 1).some((other) => touches(s, other)),
    )
  ) {
    return;
  }
  return {
    corners: points,
    segments,
    length: segments.reduce((sum, s) => sum + s.right - s.left + s.bottom - s.top, 0),
  };
}

function directRoute(source: Card, target: Card, offset: number): Route {
  const from = source.at,
    to = target.at;
  if (Math.abs(to.x - from.x) < LEAD.width && Math.abs(to.y - from.y) >= LEAD.height) {
    const below = to.y >= from.y;
    const start = { x: from.x + LEAD.width / 2 + offset, y: from.y + (below ? LEAD.height : 0) };
    const end = {
      x: to.x + LEAD.width / 2 + offset,
      y: to.y + (below ? -ARROW_GAP : LEAD.height + ARROW_GAP),
    };
    const middle = (start.y + end.y) / 2;
    return [start, { x: start.x, y: middle }, { x: end.x, y: middle }, end];
  }
  const right = to.x >= from.x;
  const start = { x: from.x + (right ? LEAD.width : 0), y: from.y + LEAD.height / 2 + offset };
  const end = {
    x: to.x + (right ? -ARROW_GAP : LEAD.width + ARROW_GAP),
    y: to.y + LEAD.height / 2 + offset,
  };
  const middle = (start.x + end.x) / 2;
  return [start, { x: middle, y: start.y }, { x: middle, y: end.y }, end];
}

function port({ at }: Card, side: Point, offset: number, gap: number) {
  const tip = {
    x: at.x + LEAD.width / 2 + side.x * (LEAD.width / 2 + gap) + Math.abs(side.y) * offset,
    y: at.y + LEAD.height / 2 + side.y * (LEAD.height / 2 + gap) + Math.abs(side.x) * offset,
  };
  return { tip, turn: { x: tip.x + side.x * TURN_CLEARANCE, y: tip.y + side.y * TURN_CLEARANCE } };
}

function* alternatives(source: Card, target: Card, offset: number, area: Bounds): Iterable<Route> {
  for (const fromSide of SIDES) {
    const from = port(source, fromSide, offset, 0);
    for (const toSide of SIDES) {
      const to = port(target, toSide, offset, ARROW_GAP);
      const head = [from.tip, from.turn],
        tail = [to.turn, to.tip];
      const a = from.turn,
        b = to.turn;
      yield [...head, { x: a.x, y: b.y }, ...tail];
      yield [...head, { x: b.x, y: a.y }, ...tail];
      for (const x of [(a.x + b.x) / 2, area.left - TURN_CLEARANCE, area.right + TURN_CLEARANCE]) {
        yield [...head, { x, y: a.y }, { x, y: b.y }, ...tail];
      }
      for (const y of [(a.y + b.y) / 2, area.top - TURN_CLEARANCE, area.bottom + TURN_CLEARANCE]) {
        yield [...head, { x: a.x, y }, { x: b.x, y }, ...tail];
      }
    }
  }
}

function better(a: Score, b: Score) {
  if (a.cards !== b.cards) return a.cards < b.cards;
  if (a.contacts !== b.contacts) return a.contacts < b.contacts;
  return a.cost < b.cost;
}

const edgeKey = (edge: AgentGraphEdge) => `${edge.source_session_id}/${edge.target_session_id}`;

/** Keep direct routes where they fit, and try other card sides before drawing a false junction. */
export function peerRoutes(
  positions: Positions,
  sessions: readonly string[],
  edges: readonly AgentGraphEdge[],
): ReadonlyMap<string, Route> {
  const cards = new Map(
    sessions.flatMap((id) => {
      const at = positions[id];
      return at
        ? [
            [
              id,
              { id, at, bounds: bounds(at, { x: at.x + LEAD.width, y: at.y + LEAD.height }) },
            ] as const,
          ]
        : [];
    }),
  );
  const keys = new Set(edges.map(edgeKey));
  const drawn: Bounds[] = [];
  const routes = new Map<string, Route>();
  for (const edge of [...edges].sort((a, b) => edgeKey(a).localeCompare(edgeKey(b)))) {
    const source = cards.get(edge.source_session_id),
      target = cards.get(edge.target_session_id);
    if (!source || !target) continue;
    const offset = keys.has(`${target.id}/${source.id}`) ? (source.id < target.id ? -8 : 8) : 0;
    const direct = directRoute(source, target, offset);
    const score = (candidate: Path): Score => {
      let blocked = 0,
        contacts = 0;
      for (const segment of candidate.segments) {
        for (const card of cards.values()) {
          if (enters(segment, card.bounds, card === source || card === target ? 0 : CARD_CLEARANCE))
            blocked += 1;
        }
        for (const other of drawn) contacts += contactCost(segment, other);
      }
      return { cards: blocked, contacts, cost: candidate.length + candidate.segments.length * 16 };
    };
    let best = path(direct);
    let bestScore = best ? score(best) : undefined;
    if (!bestScore || bestScore.cards || bestScore.contacts) {
      const from = source.bounds,
        to = target.bounds;
      const between = {
        left: Math.min(from.left, to.left),
        right: Math.max(from.right, to.right),
        top: Math.min(from.top, to.top),
        bottom: Math.max(from.bottom, to.bottom),
      };
      const area = { ...between };
      for (const card of cards.values()) {
        if (!touches(card.bounds, between)) continue;
        area.left = Math.min(area.left, card.bounds.left);
        area.right = Math.max(area.right, card.bounds.right);
        area.top = Math.min(area.top, card.bounds.top);
        area.bottom = Math.max(area.bottom, card.bounds.bottom);
      }
      for (const corners of alternatives(source, target, offset, area)) {
        const candidate = path(corners);
        if (!candidate) continue;
        const rank = score(candidate);
        if (!bestScore || better(rank, bestScore)) {
          best = candidate;
          bestScore = rank;
        }
      }
    }
    routes.set(edgeKey(edge), best?.corners ?? direct);
    if (best) drawn.push(...best.segments);
  }
  return routes;
}
