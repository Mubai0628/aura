//! Which writes run on their own session's lane instead of the connection's ordered one.
use super::lane_key::SessionLaneKey;
use super::{CONNECTION_ORDERED_ROUTES, CONTROL_ROUTES, is_concurrent_read_only};
use aura_ipc::JsonRpcRequest;

/// Session-scoped writes that carry the session they belong to.
///
/// WHY A TABLE AND NOT A PARAMETER SNIFF. Which requests may run beside another
/// session's is an ordering claim about Core state, so it is declared here once, next to
/// the lane it selects, and reviewed like any other classification. Nothing about a route
/// changes by being listed: it is still authenticated, still takes its invocation claim,
/// still runs its handler and still settles. Only *when* it may run changes - and only
/// against a different session.
///
/// WHAT IS DELIBERATELY NOT HERE
/// * `session.create`: it has no session yet, so there is nothing to order it against;
///   it stays on the ordered lane.
/// * `permission.set_defaults`, host-preparation mutations, external-grant mutations,
///   quarantine and artifact routes: global configuration or repository-wide state, not
///   one session's, so they keep the original order.
/// * `projection.subscribe` / `projection.unsubscribe`: connection state, not a session's.
pub(super) const SESSION_BOUND_WRITE_ROUTES: [&str; 17] = [
    // Sending a message and editing the pending queue of one session.
    "agent.submit",
    "agent.inbox.mutate",
    // A plugin's panel sending a message or its plugin's command in its conversation, and
    // stopping its turn. Setting the plugin's own setting is no one session's.
    "agent.plugin_session.send",
    "agent.plugin_session.run",
    "agent.plugin_session.stop",
    // One session's own run/approval/query control.
    "agent.start",
    "agent.continue",
    "agent.related.start",
    "agent.cancel",
    "agent.mode.control",
    "agent.query.answer",
    "agent.query.touch",
    "agent.query.select_file",
    "agent.context.recheck",
    "agent.approval.resolve",
    // One session's permission configuration and its own shell stop.
    "permission.set_session_mode",
    "agent.shell.stop",
];

/// Whether a write route is bound to one session, and to which one.
///
/// Read-only requests are not session writes: a read cannot observe another read's
/// ordering, and the concurrent lane already serves them. A route that names a session
/// but carries no usable `session_id` parameter is **not** put on a session lane - it
/// falls through to the ordered lane, which can only be stricter than what it asked for.
#[must_use]
pub(super) fn session_write_scope(request: &JsonRpcRequest) -> Option<SessionLaneKey> {
    if is_concurrent_read_only(request)
        || CONTROL_ROUTES.contains(&request.method.as_str())
        || CONNECTION_ORDERED_ROUTES.contains(&request.method.as_str())
    {
        return None;
    }
    if !SESSION_BOUND_WRITE_ROUTES.contains(&request.method.as_str()) {
        return None;
    }
    let session_id = request.params.get("session_id")?.as_str()?;
    if session_id.is_empty() {
        return None;
    }
    Some(SessionLaneKey::of(session_id))
}
