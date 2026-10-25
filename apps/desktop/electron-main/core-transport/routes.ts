/** The handshake budget and the internal control routes. */
/**
 * How long one handshake RPC may take when the caller does not pass a smaller budget.
 *
 * The transport-level value stays as the inner bound; a caller spending a larger
 * recovery budget passes what is left of it, and the smaller of the two wins.
 */
export const HANDSHAKE_REQUEST_TIMEOUT_MS = 10_000;

/**
 * The transport's INTERNAL control routes: the cancel and command-outcome entry points whose whole
 * purpose is to release work the business pools may be holding.
 *
 * This table is the ONLY place that asks for the reserved control admission class. The class is a
 * property of the route chosen HERE, never of the `method` string: `request()` does not consult this
 * table, so a caller - or a model that names any method, including the literal `aura.cancel` - always
 * enters the ordinary business pool and cannot spend reserved control capacity. Neither can a
 * caller-supplied priority: priority only orders the writer queue, after admission.
 *
 * Both routes keep the P0 writer priority, the per-request budgets and the frame shape they already
 * had. `commandInvocation` records that shape explicitly: `aura.cancel` is declared
 * `idempotency: command_invocation` in the generated route table and has always carried a freshly
 * minted CommandInvocationId (which is also what puts the query-without-replay hint on its failure
 * envelope), whereas `runtime.command_outcome` is keyed by the invocation it ASKS about and never
 * carried one of its own. Dropping either would silently change the bytes the Core sees.
 */
export const INTERNAL_CONTROL_ROUTES = {
  cancel: {
    commandInvocation: true,
    method: 'aura.cancel',
    priority: 0,
    timeoutMilliseconds: 15_000,
  },
  commandOutcome: {
    commandInvocation: false,
    method: 'runtime.command_outcome',
    priority: 0,
    timeoutMilliseconds: 10_000,
  },
} as const;

export type InternalControlRoute = keyof typeof INTERNAL_CONTROL_ROUTES;
