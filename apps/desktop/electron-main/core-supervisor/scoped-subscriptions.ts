import type {
  ProjectionSubscription,
  ProjectionUnsubscribeRequest,
} from '@aura/contracts/generated';

/** What the scoped subscriptions ask of Core's connection. */
interface Asking {
  request(method: string, params: unknown): Promise<unknown>;
}

/**
 * The scoped Model or Tool projection routes the windows asked Core for. A route is held from the
 * moment it is asked: a release before Core answers is then sent to Core after it on the same
 * ordered connection. Core answers every subscription with the route's current state, so a route
 * asked for again while held is asked of Core again: the page that asked has not had that state.
 * A connection made again subscribes every held route again.
 */
export class ScopedSubscriptions<Route extends object> {
  readonly #held = new Map<string, Route>();
  readonly #key: (route: Route) => string;

  constructor(key: (route: Route) => string) {
    this.#key = key;
  }

  async subscribe(
    route: Route,
    connection: () => Asking,
    knownRuntime: ProjectionSubscription['known_runtime_instance_id'],
  ): Promise<void> {
    const key = this.#key(route);
    const transport = connection();
    const before = this.#held.get(key);
    const held = before ?? structuredClone(route);
    this.#held.set(key, held);
    try {
      await transport.request('projection.subscribe', {
        ...route,
        after_revision: null,
        known_runtime_instance_id: knownRuntime,
      });
    } catch (error) {
      // Only what this subscription held: the route may have been released and asked again since.
      if (before === undefined && this.#held.get(key) === held) this.#held.delete(key);
      throw error;
    }
  }

  async unsubscribe(route: Route, transport: Asking | undefined): Promise<void> {
    if (!this.#held.delete(this.#key(route)) || transport === undefined) return;
    await transport.request('projection.unsubscribe', route as ProjectionUnsubscribeRequest);
  }

  routes(): IterableIterator<Route> {
    return this.#held.values();
  }
}
