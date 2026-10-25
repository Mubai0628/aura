//! JSON-RPC 2.0 messages as MCP uses them. A server's message is parsed strictly enough to
//! route it (request, notification, result or error) and nothing more is trusted.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    Text(String),
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(value) => write!(f, "{value}"),
            Self::Text(value) => write!(f, "{value}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Request { id: RequestId, method: String, params: Option<Value> },
    Notification { method: String, params: Option<Value> },
    Response { id: RequestId, result: Value },
    Error { id: Option<RequestId>, error: RpcError },
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum JsonRpcError {
    #[error("不是合法的 JSON：{0}")]
    Json(String),
    #[error("不是 JSON-RPC 2.0 消息")]
    NotJsonRpc,
}

fn one(value: Value) -> Result<Message, JsonRpcError> {
    let Value::Object(object) = value else {
        return Err(JsonRpcError::NotJsonRpc);
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(JsonRpcError::NotJsonRpc);
    }
    let id = match object.get("id") {
        None => None,
        Some(Value::Null) => None,
        Some(value) => Some(
            serde_json::from_value::<RequestId>(value.clone())
                .map_err(|_| JsonRpcError::NotJsonRpc)?,
        ),
    };
    if let Some(method) = object.get("method") {
        let method = method.as_str().ok_or(JsonRpcError::NotJsonRpc)?.to_owned();
        let params = object.get("params").cloned();
        return Ok(match id {
            Some(id) => Message::Request { id, method, params },
            None => Message::Notification { method, params },
        });
    }
    if let Some(error) = object.get("error") {
        let error = error.as_object().ok_or(JsonRpcError::NotJsonRpc)?;
        return Ok(Message::Error {
            id,
            error: RpcError {
                code: error.get("code").and_then(Value::as_i64).unwrap_or(-32603),
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .chars()
                    .take(2000)
                    .collect(),
                data: error.get("data").cloned(),
            },
        });
    }
    match (id, object.get("result")) {
        (Some(id), Some(result)) => Ok(Message::Response { id, result: result.clone() }),
        _ => Err(JsonRpcError::NotJsonRpc),
    }
}

/// One message, or the messages of a batch (older servers may still send one).
pub fn parse_messages(bytes: &[u8]) -> Result<Vec<Message>, JsonRpcError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| JsonRpcError::Json(error.to_string()))?;
    match value {
        Value::Array(values) if !values.is_empty() => values.into_iter().map(one).collect(),
        Value::Array(_) => Err(JsonRpcError::NotJsonRpc),
        value => Ok(vec![one(value)?]),
    }
}

fn line(value: Value) -> Vec<u8> {
    let mut bytes = value.to_string().into_bytes();
    bytes.push(b'\n');
    bytes
}

/// A request line (stdio framing; HTTP bodies drop the newline).
#[must_use]
pub fn encode_request(id: &RequestId, method: &str, params: Option<Value>) -> Vec<u8> {
    let mut object = Map::new();
    object.insert("jsonrpc".into(), json!("2.0"));
    object.insert("id".into(), json!(id));
    object.insert("method".into(), json!(method));
    if let Some(params) = params {
        object.insert("params".into(), params);
    }
    line(Value::Object(object))
}

#[must_use]
pub fn encode_notification(method: &str, params: Option<Value>) -> Vec<u8> {
    let mut object = Map::new();
    object.insert("jsonrpc".into(), json!("2.0"));
    object.insert("method".into(), json!(method));
    if let Some(params) = params {
        object.insert("params".into(), params);
    }
    line(Value::Object(object))
}

#[must_use]
pub fn encode_response(id: &RequestId, result: Value) -> Vec<u8> {
    line(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

#[must_use]
pub fn encode_error(id: &RequestId, code: i64, message: &str) -> Vec<u8> {
    line(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}))
}
