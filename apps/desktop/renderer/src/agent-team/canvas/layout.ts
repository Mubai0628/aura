/**
 * Where the agents sit on the 多 Agent canvas and how the lines between them run, in the canvas's
 * own pixels before zoom. The main agent sits above the rows its sub-agents fill in order; a line
 * leaves the main agent's bottom, turns just below it, and drops into its sub-agent's top, going
 * round any agent in the way through the gaps between them. Places never depend on the panel's
 * width: a card the user moved and one they did not stay in one frame, and the width changes
 * only the view.
 */
export const LEAD = { width: 176, height: 96 } as const;
export const MEMBER = { width: 180, height: 96 } as const;
/** The main agent's key among the positions; a sub-agent's handle never starts with '@'. */
export const LEAD_KEY = '@lead';

/** How many sub-agents a row holds. */
export const ROW_LENGTH = 4;
const GAP_X = 20;
const ROW_STEP = MEMBER.height + 48;
const TOP = 30;
const FIRST_ROW = TOP + LEAD.height + 64;
const MARGIN = 16;
/** How far above a sub-agent, or below the main agent, a line turns. */
const TURN = 24;

export interface Point {
  x: number;
  y: number;
}
export type Positions = Readonly<Record<string, Point>>;

/** A line's corners from its first end to its last. */
export type Route = readonly Point[];

interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

const boxOf = (key: string, at: Point): Box => ({
  ...at,
  ...(key === LEAD_KEY ? LEAD : MEMBER),
});

/** The main agent's middle: over a full row, which starts at the canvas's margin. */
const MIDDLE = MARGIN + (ROW_LENGTH * MEMBER.width + (ROW_LENGTH - 1) * GAP_X) / 2;

/** Where the main agent and each sub-agent start: the main agent in the same place however many
 *  sub-agents there are, and their rows centred under it. */
export function defaultLayout(agents: readonly string[]): Record<string, Point> {
  const columns = Math.min(ROW_LENGTH, Math.max(1, agents.length));
  const rowWidth = columns * MEMBER.width + (columns - 1) * GAP_X;
  const left = MIDDLE - rowWidth / 2;
  const placed: Record<string, Point> = {
    [LEAD_KEY]: { x: Math.round(MIDDLE - LEAD.width / 2), y: TOP },
  };
  agents.forEach((agent, index) => {
    placed[agent] = {
      x: Math.round(left + (index % columns) * (MEMBER.width + GAP_X)),
      y: FIRST_ROW + Math.floor(index / columns) * ROW_STEP,
    };
  });
  return placed;
}

/** Whether the upright line at `x` from `top` to `bottom` crosses one of `boxes`. */
function blocksUpright(boxes: readonly Box[], x: number, top: number, bottom: number): boolean {
  return boxes.some(
    (box) =>
      x > box.x - 1 && x < box.x + box.width + 1 && bottom > box.y && top < box.y + box.height,
  );
}

/**
 * The line from the main agent to sub-agent `agent`. Everything else on the canvas is in the way
 * of its upright parts; a sub-agent beside the main agent is joined side to side.
 */
export function route(positions: Positions, agent: string): Route {
  const lead = boxOf(LEAD_KEY, positions[LEAD_KEY] ?? { x: 0, y: 0 });
  const member = boxOf(agent, positions[agent] ?? { x: 0, y: 0 });
  const others = Object.entries(positions)
    .filter(([key]) => key !== agent && key !== LEAD_KEY)
    .map(([key, at]) => boxOf(key, at));
  const leadX = lead.x + lead.width / 2;
  const memberX = member.x + member.width / 2;
  const below = member.y >= lead.y + lead.height + 2 * TURN;
  const above = member.y + member.height <= lead.y - 2 * TURN;
  if (!below && !above) {
    const leadY = lead.y + lead.height / 2;
    const memberY = member.y + member.height / 2;
    const right = memberX >= leadX;
    const from = right ? lead.x + lead.width : lead.x;
    const to = right ? member.x : member.x + member.width;
    const middle = (from + to) / 2;
    return [
      { x: from, y: leadY },
      { x: middle, y: leadY },
      { x: middle, y: memberY },
      { x: to, y: memberY },
    ];
  }
  const leadEnd = below ? lead.y + lead.height : lead.y;
  const memberEnd = below ? member.y : member.y + member.height;
  const step = below ? 1 : -1;
  const bus = leadEnd + step * Math.min(TURN, Math.abs(memberEnd - leadEnd) / 2);
  const turn = memberEnd - step * TURN;
  const [top, bottom] = below ? [bus, memberEnd] : [memberEnd, bus];
  if (!blocksUpright(others, memberX, top, bottom)) {
    return [
      { x: leadX, y: leadEnd },
      { x: leadX, y: bus },
      { x: memberX, y: bus },
      { x: memberX, y: memberEnd },
    ];
  }
  // Round the agents in the way: down the gap beside the sub-agent nearer the main agent, else
  // the other one.
  const gaps = [member.x + member.width + GAP_X / 2, member.x - GAP_X / 2].sort(
    (a, b) => Math.abs(a - leadX) - Math.abs(b - leadX),
  );
  const [laneTop, laneBottom] = below ? [bus, turn] : [turn, bus];
  const lane = gaps.find((x) => !blocksUpright(others, x, laneTop, laneBottom)) ?? gaps[0]!;
  return [
    { x: leadX, y: leadEnd },
    { x: leadX, y: bus },
    { x: lane, y: bus },
    { x: lane, y: turn },
    { x: memberX, y: turn },
    { x: memberX, y: memberEnd },
  ];
}

/** A route as an SVG path. */
export const pathOf = (corners: Route): string =>
  corners
    .map((at, index) => `${index ? 'L' : 'M'}${Math.round(at.x)} ${Math.round(at.y)}`)
    .join(' ');

/** The middle of a route's longest level part, where a label about it sits. */
export function labelPoint(corners: Route): Point {
  let best = corners[0] ?? { x: 0, y: 0 };
  let length = -1;
  for (let index = 1; index < corners.length; index += 1) {
    const [a, b] = [corners[index - 1]!, corners[index]!];
    if (a.y === b.y && Math.abs(b.x - a.x) > length) {
      length = Math.abs(b.x - a.x);
      best = { x: (a.x + b.x) / 2, y: a.y };
    }
  }
  return best;
}

export interface Camera {
  x: number;
  y: number;
  zoom: number;
}

export const MIN_ZOOM = 0.4;
export const MAX_ZOOM = 2;
export const clampZoom = (zoom: number, minimum = MIN_ZOOM) =>
  Math.min(MAX_ZOOM, Math.max(minimum, zoom));

/** The camera that shows every agent on a board this big, at most at full size, from the top. */
export function fit(
  positions: Positions,
  board: { width: number; height: number },
  minimum = MIN_ZOOM,
): Camera {
  const boxes = Object.entries(positions).map(([key, at]) => boxOf(key, at));
  if (!boxes.length || board.width <= 0 || board.height <= 0) return { x: 0, y: 0, zoom: 1 };
  // Room for a branch tab above a sub-agent and an approval mark at its corner.
  const left = Math.min(...boxes.map((box) => box.x)) - 12;
  const top = Math.min(...boxes.map((box) => box.y)) - 24;
  const right = Math.max(...boxes.map((box) => box.x + box.width)) + 6;
  const bottom = Math.max(...boxes.map((box) => box.y + box.height)) + 6;
  const zoom = clampZoom(
    Math.min(
      1,
      (board.width - 2 * MARGIN) / (right - left),
      (board.height - 2 * MARGIN) / (bottom - top),
    ),
    minimum,
  );
  return {
    x: Math.round((board.width - (right - left) * zoom) / 2 - left * zoom),
    y: Math.round(MARGIN - top * zoom),
    zoom,
  };
}

/** The camera zoomed by `factor` about the point `at` of the board. */
export function zoomAbout(camera: Camera, factor: number, at: Point, minimum = MIN_ZOOM): Camera {
  const zoom = clampZoom(camera.zoom * factor, minimum);
  const scale = zoom / camera.zoom;
  return { x: at.x - (at.x - camera.x) * scale, y: at.y - (at.y - camera.y) * scale, zoom };
}
