//! A terminal's input and lifecycle share one bounded, ordered lane.
use super::{JsonRpcRequest, typed_params};
use aura_domain_common::SessionId;
use aura_windows_user_contract::{
    TerminalControlRequestV1, TerminalRequestV1, TerminalResizeRequestV1,
    UserTerminalWriteRequestV1,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TerminalLaneKey {
    session: SessionId,
    terminal: String,
}
pub(super) fn scope(request: &JsonRpcRequest) -> Option<TerminalLaneKey> {
    let (session, terminal) = match request.method.as_str() {
        "terminal.user.write" => {
            let input = typed_params::<UserTerminalWriteRequestV1>(request)?;
            (input.session_id, input.terminal_id)
        }
        "terminal.user.control" => {
            let input = typed_params::<TerminalControlRequestV1>(request)?;
            (input.session_id, input.terminal_id)
        }
        "terminal.user.resize" => {
            let input = typed_params::<TerminalResizeRequestV1>(request)?;
            (input.session_id, input.terminal_id)
        }
        "terminal.user.close" => {
            let input = typed_params::<TerminalRequestV1>(request)?;
            (input.session_id, input.terminal_id)
        }
        _ => return None,
    };
    if terminal.is_empty() {
        return None;
    }
    Some(TerminalLaneKey { session, terminal })
}
