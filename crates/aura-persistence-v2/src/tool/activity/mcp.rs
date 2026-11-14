//! The session row of an MCP call names the server that handled it and says that Aura does
//! not check what the server did. A call that never reached the server states its refusal alone.
use aura_execution_contract::{
    AttemptState, ExecutionAttempt, SecuredOperation, SideEffectStateV1,
};
use aura_persistence_core::{StoreResult, record};
use aura_security_contract::SecuritySourceRef;
use aura_tool_projection::AgentToolDisplay;
use rusqlite::Connection;

/// An MCP call's row title: what the call is, then the tool's name on its server. Server ids
/// and tool segments never contain a dot, so the canonical name splits in three.
pub(super) fn title(name: &str) -> Option<String> {
    let mut parts = name.splitn(3, '.');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("mcp"), Some(server), Some(tool)) if !server.is_empty() && !tool.is_empty() => {
            Some(format!("调用 MCP 工具 {tool}"))
        }
        _ => None,
    }
}

pub(super) fn notice(server_id: &str) -> String {
    format!("由 MCP 服务器“{server_id}”执行，Aura 不检查它做了什么")
}

pub(super) fn decorate(
    db: &Connection,
    mut view: AgentToolDisplay,
    operation: Option<&SecuredOperation>,
) -> StoreResult<AgentToolDisplay> {
    let Some(operation) = operation else {
        return Ok(view);
    };
    let SecuritySourceRef::McpServer { server_id } = &operation.content.source else {
        return Ok(view);
    };
    let mut attempts = db
        .prepare("SELECT payload FROM execution_attempts WHERE operation_id=?1 ORDER BY ordinal")?;
    let payloads = attempts
        .query_map([operation.operation_id.to_string()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut reached = false;
    for payload in payloads {
        let attempt: ExecutionAttempt = record::decode(&payload)?;
        // A running call is on its way to the server; a finished one reached it unless Core
        // recorded that nothing was sent.
        reached |= match attempt.state {
            AttemptState::Terminal { result } => result.side_effect != SideEffectStateV1::NoEffect,
            _ => true,
        };
    }
    if reached {
        if !view.summary.is_empty() {
            view.summary.push('；');
        }
        view.summary.push_str(&notice(server_id));
    }
    Ok(view)
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_mcp_row_is_titled_with_the_tool_on_its_server() {
        assert_eq!(super::title("mcp.files.echo").as_deref(), Some("调用 MCP 工具 echo"));
        assert_eq!(
            super::title("mcp.livecheck.get_weather").as_deref(),
            Some("调用 MCP 工具 get_weather")
        );
        for name in ["mcp.files", "mcp..echo", "fs.read", "memory.create"] {
            assert_eq!(super::title(name), None, "{name}");
        }
    }
}
