// @vitest-environment node
import { expect, test, vi } from 'vitest';
import type { CoreSupervisor } from './core-supervisor.js';
import { createProjectionRoutes } from './main-projection-routes.js';
import {
  modelProjectionRouteKey,
  toolProjectionRouteKey,
  type ModelProjectionRouteRequest,
  type ToolProjectionRouteRequest,
} from '../shared/projection-validation.js';

vi.mock('./core-supervisor.js', () => ({ CoreSupervisor: class {} }));

// A page that loads again in its window (a reload, a dev server's refresh) shows the same routes
// again. Main publishes a route's changes only to a window that owns it, and Core sends a route's
// current state only in answer to a subscription: the new page has to take each route afresh, even
// while Core is still answering the release of the old page's routes.

const S1 = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const S2 = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW';
const model = (session: string) =>
  ({
    projection_kind: 'session_model_selector',
    projection_scope: { type: 'session', session_id: session },
  }) as ModelProjectionRouteRequest;
const tool = (session: string) =>
  ({
    projection_kind: 'session_loaded_tools',
    projection_scope: { type: 'session', session_id: session },
  }) as ToolProjectionRouteRequest;

function setup() {
  const sent: string[] = [];
  const answer = Promise.withResolvers<void>();
  let slow = false;
  const told = (what: string) => async () => {
    sent.push(what);
    if (slow && what.startsWith('unsubscribe')) await answer.promise;
  };
  const supervisor = {
    subscribeModelProjection: vi.fn((route: ModelProjectionRouteRequest) =>
      told(`subscribe ${modelProjectionRouteKey(route)}`)(),
    ),
    unsubscribeModelProjection: vi.fn((route: ModelProjectionRouteRequest) =>
      told(`unsubscribe ${modelProjectionRouteKey(route)}`)(),
    ),
    subscribeToolProjection: vi.fn((route: ToolProjectionRouteRequest) =>
      told(`subscribe ${toolProjectionRouteKey(route)}`)(),
    ),
    unsubscribeToolProjection: vi.fn((route: ToolProjectionRouteRequest) =>
      told(`unsubscribe ${toolProjectionRouteKey(route)}`)(),
    ),
  };
  const state = {
    supervisor: () => supervisor as unknown as CoreSupervisor,
    appendMainLog: vi.fn(async () => undefined),
    modelProjectionRoutesByWebContents: new Map<number, Set<string>>(),
    modelProjectionRoutes: new Map<
      string,
      { owners: Set<number>; route: ModelProjectionRouteRequest }
    >(),
    toolProjectionRoutesByWebContents: new Map<number, Set<string>>(),
    toolProjectionRoutes: new Map<
      string,
      { owners: Set<number>; route: ToolProjectionRouteRequest }
    >(),
  };
  return {
    routes: createProjectionRoutes(state),
    state,
    sent,
    /** Core answers the releases only when `answer` is called. */
    slowCore: () => {
      slow = true;
    },
    answer: () => answer.resolve(),
  };
}

test('a page loaded again takes its model routes afresh while the old page’s are released', async () => {
  const t = setup();
  const [a, b] = [model(S1), model(S2)];
  const [ka, kb] = [modelProjectionRouteKey(a), modelProjectionRouteKey(b)];
  await t.routes.subscribeModelProjectionRoute(1, a);
  await t.routes.subscribeModelProjectionRoute(1, b);
  t.slowCore();
  const released = t.routes.releaseModelProjectionRoutes(1);
  await t.routes.subscribeModelProjectionRoute(1, b);
  t.answer();
  await released;
  expect(t.sent).toEqual([
    `subscribe ${ka}`,
    `subscribe ${kb}`,
    `unsubscribe ${ka}`,
    `unsubscribe ${kb}`,
    `subscribe ${kb}`,
  ]);
  expect(t.state.modelProjectionRoutesByWebContents.get(1)).toEqual(new Set([kb]));
  expect([...t.state.modelProjectionRoutes.keys()]).toEqual([kb]);
  expect(t.state.modelProjectionRoutes.get(kb)?.owners).toEqual(new Set([1]));
});

test('a page loaded again takes its tool routes afresh while the old page’s are released', async () => {
  const t = setup();
  const [a, b] = [tool(S1), tool(S2)];
  const [ka, kb] = [toolProjectionRouteKey(a), toolProjectionRouteKey(b)];
  await t.routes.subscribeToolProjectionRoute(1, a);
  await t.routes.subscribeToolProjectionRoute(1, b);
  t.slowCore();
  const released = t.routes.releaseToolProjectionRoutes(1);
  await t.routes.subscribeToolProjectionRoute(1, b);
  t.answer();
  await released;
  expect(t.sent).toEqual([
    `subscribe ${ka}`,
    `subscribe ${kb}`,
    `unsubscribe ${ka}`,
    `unsubscribe ${kb}`,
    `subscribe ${kb}`,
  ]);
  expect(t.state.toolProjectionRoutesByWebContents.get(1)).toEqual(new Set([kb]));
  expect(t.state.toolProjectionRoutes.get(kb)?.owners).toEqual(new Set([1]));
});

test('a page that goes away releases both its model and its tool routes', async () => {
  const t = setup();
  await t.routes.subscribeModelProjectionRoute(1, model(S1));
  await t.routes.subscribeToolProjectionRoute(1, tool(S1));
  await t.routes.releasePage(1);
  expect(t.sent.slice(2).sort()).toEqual(
    [
      `unsubscribe ${modelProjectionRouteKey(model(S1))}`,
      `unsubscribe ${toolProjectionRouteKey(tool(S1))}`,
    ].sort(),
  );
  expect(t.state.modelProjectionRoutesByWebContents.size).toBe(0);
  expect(t.state.toolProjectionRoutesByWebContents.size).toBe(0);
  expect(t.state.modelProjectionRoutes.size + t.state.toolProjectionRoutes.size).toBe(0);
});

// Seen in the development app: the old page asked for its route just before it began to load
// again, and Main took that request after releasing the page's routes. The window then held the
// route, Core's answer went to the page that was leaving, and the new page's own request found the
// route held and was not passed on, so the new page never had the route's state.
test('a route a window holds already is asked of Core again for the page that asks', async () => {
  const t = setup();
  const [a, b] = [model(S1), tool(S1)];
  await t.routes.releasePage(1);
  await t.routes.subscribeModelProjectionRoute(1, a);
  await t.routes.subscribeToolProjectionRoute(1, b);
  await t.routes.subscribeModelProjectionRoute(1, a);
  await t.routes.subscribeToolProjectionRoute(1, b);
  const [ka, kb] = [modelProjectionRouteKey(a), toolProjectionRouteKey(b)];
  expect(t.sent).toEqual([
    `subscribe ${ka}`,
    `subscribe ${kb}`,
    `subscribe ${ka}`,
    `subscribe ${kb}`,
  ]);
  expect(t.state.modelProjectionRoutes.get(ka)?.owners).toEqual(new Set([1]));
  expect(t.state.toolProjectionRoutes.get(kb)?.owners).toEqual(new Set([1]));
  // One release still lets go of each route once.
  await t.routes.unsubscribeModelProjectionRoute(1, a);
  await t.routes.unsubscribeToolProjectionRoute(1, b);
  expect(t.sent.slice(4)).toEqual([`unsubscribe ${ka}`, `unsubscribe ${kb}`]);
  expect(t.state.modelProjectionRoutes.size + t.state.toolProjectionRoutes.size).toBe(0);
});
