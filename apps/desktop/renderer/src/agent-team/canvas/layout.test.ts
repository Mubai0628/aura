import { expect, test } from 'vitest';
import {
  LEAD,
  LEAD_KEY,
  MEMBER,
  ROW_LENGTH,
  defaultLayout,
  fit,
  labelPoint,
  pathOf,
  route,
  zoomAbout,
  type Positions,
} from './layout';

// Where the agents sit on the canvas and how the lines between them run.

const boxes = (positions: Positions) =>
  Object.entries(positions).map(([key, at]) => ({
    key,
    ...at,
    ...(key === LEAD_KEY ? LEAD : MEMBER),
  }));

/** Whether a route's upright and level parts stay out of every card but its two ends. */
function clear(positions: Positions, agent: string) {
  const corners = route(positions, agent);
  const others = boxes(positions).filter((box) => box.key !== agent && box.key !== LEAD_KEY);
  for (let index = 1; index < corners.length; index += 1) {
    const [a, b] = [corners[index - 1]!, corners[index]!];
    for (const box of others) {
      const left = Math.min(a.x, b.x);
      const right = Math.max(a.x, b.x);
      const top = Math.min(a.y, b.y);
      const bottom = Math.max(a.y, b.y);
      const crosses =
        right > box.x && left < box.x + box.width && bottom > box.y && top < box.y + box.height;
      if (crosses) return false;
    }
  }
  return true;
}

test('where the agents start depends on the agents alone: four a row under the main agent', () => {
  expect(ROW_LENGTH).toBe(4);
  const agents = ['A1', 'A2', 'A3', 'A4', 'A5', 'A6'];
  const placed = defaultLayout(agents);
  const lead = placed[LEAD_KEY]!;
  for (const agent of ['A2', 'A3', 'A4']) expect(placed[agent]!.y).toBe(placed.A1!.y);
  expect(placed.A5!.y).toBeGreaterThan(placed.A1!.y + MEMBER.height);
  expect(placed.A5!.x).toBe(placed.A1!.x);
  expect(placed.A6!.x).toBe(placed.A2!.x);
  // The main agent is centred over the first row, and every place is on the canvas.
  const row = placed.A4!.x + MEMBER.width - placed.A1!.x;
  expect(lead.x + LEAD.width / 2).toBeCloseTo(placed.A1!.x + row / 2, 0);
  expect(lead.y + LEAD.height).toBeLessThan(placed.A1!.y);
  for (const at of Object.values(placed)) expect(Math.min(at.x, at.y)).toBeGreaterThanOrEqual(0);
  // A team that grows keeps the main agent where it was, its row centred under it.
  for (const count of [1, 2, 3, 5]) {
    const grown = defaultLayout(agents.slice(0, count));
    expect(grown[LEAD_KEY]).toEqual(lead);
    const first = Math.min(count, ROW_LENGTH);
    const left = grown.A1!.x;
    const right = grown[`A${first}`]!.x + MEMBER.width;
    expect((left + right) / 2).toBeCloseTo(lead.x + LEAD.width / 2, 0);
  }
});

test('a line leaves the main agent, turns below it, and goes round the cards in its way', () => {
  for (const count of [1, 2, 4, 5, 7, 9]) {
    const agents = Array.from({ length: count }, (_, index) => `A${index + 1}`);
    const placed = defaultLayout(agents);
    for (const agent of agents) expect(clear(placed, agent), `${agent} of ${count}`).toBe(true);
  }
  const placed = defaultLayout(['A1', 'A2']);
  const first = route(placed, 'A1');
  // It starts at the main agent's bottom and ends at the sub-agent's top.
  expect(first[0]).toEqual({
    x: placed[LEAD_KEY]!.x + LEAD.width / 2,
    y: placed[LEAD_KEY]!.y + LEAD.height,
  });
  expect(first.at(-1)).toEqual({ x: placed.A1!.x + MEMBER.width / 2, y: placed.A1!.y });
  expect(pathOf(first)).toMatch(/^M\d+ \d+( L\d+ \d+)+$/u);
});

test('a sub-agent moved beside the main agent is joined side to side, and one above from the top', () => {
  const beside: Positions = { [LEAD_KEY]: { x: 0, y: 100 }, A1: { x: 400, y: 90 } };
  const side = route(beside, 'A1');
  expect(side[0]).toEqual({ x: LEAD.width, y: 100 + LEAD.height / 2 });
  expect(side.at(-1)).toEqual({ x: 400, y: 90 + MEMBER.height / 2 });
  const above: Positions = { [LEAD_KEY]: { x: 0, y: 400 }, A1: { x: 0, y: 0 } };
  const up = route(above, 'A1');
  expect(up[0]!.y).toBe(400);
  expect(up.at(-1)!.y).toBe(MEMBER.height);
});

test('a label sits on the middle of the longest level part', () => {
  expect(
    labelPoint([
      { x: 0, y: 0 },
      { x: 0, y: 10 },
      { x: 100, y: 10 },
      { x: 100, y: 40 },
    ]),
  ).toEqual({ x: 50, y: 10 });
});

test('the camera keeps every card in sight, at most at full size, and zooms about a point', () => {
  const placed = defaultLayout(['A1', 'A2', 'A3', 'A4', 'A5', 'A6']);
  const board = { width: 412, height: 300 };
  const camera = fit(placed, board);
  expect(camera.zoom).toBeLessThan(1);
  for (const box of boxes(placed)) {
    expect(box.x * camera.zoom + camera.x).toBeGreaterThanOrEqual(0);
    expect((box.x + box.width) * camera.zoom + camera.x).toBeLessThanOrEqual(board.width);
    expect(box.y * camera.zoom + camera.y).toBeGreaterThanOrEqual(0);
    expect((box.y + box.height) * camera.zoom + camera.y).toBeLessThanOrEqual(board.height);
  }
  expect(fit(defaultLayout(['A1']), { width: 412, height: 900 }).zoom).toBe(1);
  // The point under the pointer stays put.
  const zoomed = zoomAbout({ x: 10, y: 20, zoom: 1 }, 2, { x: 110, y: 120 });
  expect(zoomed).toEqual({ x: -90, y: -80, zoom: 2 });
  expect(zoomAbout({ x: 0, y: 0, zoom: 1 }, 100, { x: 0, y: 0 }).zoom).toBe(2);
  expect(zoomAbout({ x: 0, y: 0, zoom: 1 }, 0.01, { x: 0, y: 0 }).zoom).toBe(0.4);
});
