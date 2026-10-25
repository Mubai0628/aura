/**
 * Two questions the Renderer must answer about a failed request, kept in one place because the
 * answer decides whether a failure is displayed as "this one call did not finish" or as the
 * connection itself being gone — and whether a command may be sent again at all.
 *
 * Both are read from the transport's own envelope fields, never guessed from a message.
 */

/**
 * The transport's code for "this request stopped waiting". Since request lifetime is owned per
 * request, this settles one waiter: the channel stays open, the other in-flight requests are
 * untouched, and the Core process is not ended. It is therefore NOT a channel terminal.
 */
export const REQUEST_TIMEOUT_CODE = 'ipc.request_timeout';

/**
 * The transport's own declaration, inside `details`, that the command may already have reached
 * Core. A caller that reads it must obtain the authoritative result from
 * `runtime.command_outcome` instead of replaying the command.
 */
export const OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY = 'query_runtime.command_outcome_without_replay';

/**
 * The transport's own declaration, inside `details`, that no byte of the request ever reached the
 * writer, so the command cannot have run. It is the opposite of the query hint: there is nothing to
 * query, the failure is definite, and it must not be shown as a possibly-executed command.
 */
export const OUTCOME_RECOVERY_NOT_SENT = 'not_sent';
/** Interactive input has no durable command outcome; inspect state and never replay it. */
export const OUTCOME_RECOVERY_TERMINAL_INPUT_NO_REPLAY = 'terminal_input_unconfirmed_do_not_replay';

/**
 * Core's own classified code for "this command started and its outcome is not known".
 *
 * It is the code Core puts on the envelope it produces when a synchronous business operation did
 * not answer inside its budget (`command_settlement::outcome_unknown_envelope`). A caller that sees
 * it is being told the same thing the transport's query hint says - resolve the invocation through
 * `runtime.command_outcome`, never replay it - so it belongs to the same classification. It is a
 * contract code, not a message: the check below never reads the text.
 */
export const COMMAND_OUTCOME_UNKNOWN_CODE = 'command.outcome_unknown';

export interface IpcFailureLike {
  code: string;
  details?: Record<string, unknown> | undefined;
  terminal: string;
}

/** Terminals that mean the CHANNEL ended, as opposed to one request being settled locally. */
const CHANNEL_TERMINALS = new Set(['transport_lost', 'core_restarted']);

/** Bridge-level codes that also leave the outcome of the command unconfirmed. */
const UNCONFIRMED_CODES = new Set(['agent_transport_failed', 'invalid_agent_result']);

/**
 * True when only this request failed: the channel was not reported lost and Core was not restarted.
 * A caller showing a connection problem for this is describing something that did not happen.
 */
export const isRequestLocalTimeout = (error: IpcFailureLike): boolean =>
  error.code === REQUEST_TIMEOUT_CODE && !CHANNEL_TERMINALS.has(error.terminal);

/**
 * True when a command may have run without the caller receiving a terminal for it. Such a result is
 * not a definite failure: it has to be shown as pending confirmation and the command must not be
 * replayed. The authoritative result is `runtime.command_outcome`.
 *
 * Two independent typed statements put a failure here, and neither is the message text:
 *  - the transport's own `OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY` declaration, which is what Main's
 *    confirmation entry reads to decide the invocation may be queried; and
 *  - Core's `COMMAND_OUTCOME_UNKNOWN_CODE`, which is the code Core itself classifies a started but
 *    unanswered command with. Core emits it with `terminal: failed` (the request's terminal is
 *    genuinely failed - there is no result), so a rule that only looked at the terminal would read
 *    a started command as a definite failure. Recognising the code is what keeps that from
 *    happening, and it is what makes a real Core envelope - not only a transport-made timeout -
 *    reach the pending-confirmation path.
 *
 * A transport that declares `OUTCOME_RECOVERY_NOT_SENT` is making the opposite statement - no byte
 * reached the writer - so those failures stay definite and are deliberately not unconfirmed.
 */
export const isUnconfirmedCommandOutcome = (error: IpcFailureLike): boolean => {
  const recovery = error.details?.outcome_recovery;
  // An explicit declaration is read first, in both directions. `not_sent` says no byte reached the
  // writer, so this command's outcome is definite whatever the channel did; the query hint says the
  // opposite, even when the request's own terminal is `failed`.
  if (recovery === OUTCOME_RECOVERY_NOT_SENT) return false;
  if (recovery === OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY) return true;
  if (recovery === OUTCOME_RECOVERY_TERMINAL_INPUT_NO_REPLAY) return true;
  return (
    error.code === COMMAND_OUTCOME_UNKNOWN_CODE ||
    error.terminal === 'outcome_unknown' ||
    CHANNEL_TERMINALS.has(error.terminal) ||
    UNCONFIRMED_CODES.has(error.code)
  );
};
