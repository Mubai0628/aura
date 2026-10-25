//! What an activity row says about the user's network proxy. Only the four network tools
//! and package HTTP tools record a route, so no other tool
//! reads its evidence for one.
use crate::tool::execution::CallExecution;
use aura_domain_common::SecurityOperationId;
use aura_persistence_core::artifact;
use aura_tool_projection::AgentToolDisplay;
use rusqlite::Connection;
use serde_json::Value;

/// “经代理” only when a hop actually used the user proxy. When the proxy could not start a
/// request and the hop went direct, the row says so: a stopped proxy is never silent, even
/// when the direct attempt succeeds and the model has nothing to report.
pub(super) fn append_proxy_detail(
    db: &Connection,
    call: &CallExecution,
    name: &str,
    view: &mut AgentToolDisplay,
) {
    if !matches!(name, "http.request" | "http.download" | "web.search" | "web.fetch") {
        return;
    }
    let Ok(operation) = db.query_row(
        "SELECT operation_id FROM tool_calls WHERE id=?1",
        [call.call.tool_call_id.to_string()],
        |row| row.get::<_, String>(0),
    ) else {
        return;
    };
    let Ok(operation) = operation.parse::<SecurityOperationId>() else {
        return;
    };
    append_route_detail(db, operation, view);
}

/// The route lines of one operation that went through Core's controlled HTTP.
pub(super) fn append_route_detail(
    db: &Connection,
    operation: SecurityOperationId,
    view: &mut AgentToolDisplay,
) {
    let route = route_evidence(db, operation);
    if used_user_proxy(&route) {
        view.details.push("经代理".into());
    }
    if let Some(line) = direct_recovery(&route) {
        view.details.push(line);
    }
}

fn route_evidence(db: &Connection, operation: SecurityOperationId) -> Value {
    let Ok(attempt) = crate::execution::load(db, operation, 1) else {
        return Value::Null;
    };
    let aura_execution_contract::AttemptState::Terminal { result } = &attempt.state else {
        return Value::Null;
    };
    let Ok(bytes) = artifact::read(db, result.receipt.evidence_ref, &operation.to_string()) else {
        return Value::Null;
    };
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|evidence| evidence.get("network_route").cloned())
        .unwrap_or(Value::Null)
}

fn used_user_proxy(route: &Value) -> bool {
    route.as_array().is_some_and(|hops| {
        hops.iter()
            .any(|hop| hop.pointer("/route/kind").and_then(Value::as_str) == Some("user_proxy"))
    })
}

/// The first hop that went direct because the proxy could not start a request.
fn direct_recovery(route: &Value) -> Option<String> {
    let failure = route.as_array()?.iter().find_map(|hop| hop.get("proxy_failure"))?;
    let host = failure.get("host")?.as_str()?;
    let port = failure.get("port")?.as_u64()?;
    let reason = failure.get("reason").and_then(Value::as_str).unwrap_or("连接失败");
    Some(format!("代理 {host}:{port} 不可用：{reason}；已改走直连"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_proxy_hop_is_named_and_a_direct_fallback_says_why() {
        let proxied = serde_json::json!([{
            "route": {"kind": "user_proxy", "protocol": "socks5", "host": "127.0.0.1", "port": 10808}
        }]);
        let direct = serde_json::json!([{
            "route": {"kind": "direct"},
            "proxy_failure": {"kind": "unavailable", "protocol": "http", "host": "127.0.0.1", "port": 1, "reason": "拒绝连接"}
        }]);
        assert!(used_user_proxy(&proxied));
        assert!(direct_recovery(&proxied).is_none());
        assert!(!used_user_proxy(&direct));
        assert_eq!(
            direct_recovery(&direct).as_deref(),
            Some("代理 127.0.0.1:1 不可用：拒绝连接；已改走直连")
        );
        assert!(!used_user_proxy(&Value::Null));
        assert!(direct_recovery(&Value::Null).is_none());
    }
}
