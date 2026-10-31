//! One HTTP exchange through Core's network gateway, and reading its answer from a JSON body or
//! an event stream. The server's pings are answered; its other requests are refused (Aura
//! declares no client capability).
use super::failure::{Failure, classify, describe};
use super::session::{Answer, CANCEL_GRACE, HttpSession, current};
use crate::mcp::credentials::RequestCredential;
use aura_domain_common::{CorrelationId, NetworkRequestId, PrincipalId, UtcTimestamp};
use aura_mcp::{
    MAX_MESSAGE_BYTES, Message, RequestId, SseEvent, SseParser, encode_error, encode_response,
    parse_messages,
};
use aura_network_control::{GatewayRequest, GatewayStreamResponse, McpHttpRequestGrant};
use aura_permission_domain::{NetworkByteLimits, NetworkRequestPlan, RedirectPolicy};
use aura_security_contract::{
    InvocationActor, InvocationIdentity, NetworkCallerKind, NetworkDataClass,
};
use serde_json::json;
use std::{sync::atomic::Ordering, time::Duration};

/// A response stream may carry the server's notes and pings before the answer itself; each
/// message is still limited to [`MAX_MESSAGE_BYTES`].
const MAX_RESPONSE_BYTES: usize = 4 * MAX_MESSAGE_BYTES;

fn limits() -> NetworkByteLimits {
    let message = u64::try_from(MAX_MESSAGE_BYTES).unwrap_or(u64::MAX);
    let response = u64::try_from(MAX_RESPONSE_BYTES).unwrap_or(u64::MAX);
    NetworkByteLimits {
        max_upload_bytes: message,
        max_download_bytes: response,
        max_decompressed_bytes: response,
    }
}

impl HttpSession {
    /// Sends `body` to the trusted address under a grant issued for exactly this request.
    pub(super) async fn send(
        &self,
        method: &str,
        body: Vec<u8>,
        timeout: Duration,
        credential: Option<&RequestCredential>,
    ) -> Result<GatewayStreamResponse, Failure> {
        let plan = NetworkRequestPlan {
            request_id: NetworkRequestId::new(),
            identity: InvocationIdentity {
                principal_id: PrincipalId::new(),
                actor: InvocationActor::SystemRuntime,
                workspace_id: None,
                session_id: None,
                turn_task_id: None,
                agent_run_id: None,
                parent_agent_run_id: None,
                correlation_id: CorrelationId::new(),
                permission_snapshot_id: None,
            },
            caller: NetworkCallerKind::McpHttp,
            method: method.into(),
            requested_url: self.plan.endpoint.clone(),
            destination: None,
            request_data: Vec::new(),
            // Tool arguments are whatever the model wrote.
            data_classes: vec![NetworkDataClass::Unknown],
            redirect_policy: RedirectPolicy::Deny,
            byte_limits: limits(),
            // The caller's own deadline ends the exchange first.
            timeout_ms: u64::try_from(timeout.as_millis())
                .unwrap_or(u64::MAX)
                .saturating_add(5_000),
            credential_refs: Vec::new(),
            network_grant_ids: Vec::new(),
        };
        let mut headers =
            vec![("accept".to_owned(), "application/json, text/event-stream".to_owned())];
        if method == "POST" {
            headers.push(("content-type".to_owned(), "application/json".to_owned()));
        }
        if let Some(version) = current(&self.protocol) {
            headers.push(("mcp-protocol-version".to_owned(), version));
        }
        if let Some(session) = current(&self.session_id) {
            headers.push(("mcp-session-id".to_owned(), session));
        }
        headers.extend(self.plan.headers.iter().cloned());
        let request = GatewayRequest { plan, headers, body };
        let configured: Vec<String> =
            self.plan.headers.iter().map(|(name, _)| name.clone()).collect();
        let seconds = i64::try_from(timeout.as_secs()).unwrap_or(i64::MAX / 2).saturating_add(60);
        let expires_at = UtcTimestamp::from_value(
            UtcTimestamp::now().value() + time::Duration::seconds(seconds),
        );
        let grant = McpHttpRequestGrant::issue_with_credential(
            &self.plan.endpoint,
            &configured,
            self.plan.network_proxy.clone(),
            credential.map(|credential| credential.binding.clone()),
            &request,
            expires_at,
        )
        .map_err(|error| Failure::NotSent(describe(&error)))?;
        let resolver = credential.map(|credential| {
            &credential.resolver as &dyn aura_network_control::ToolCredentialResolver
        });
        self.gateway.stream_with_credentials(grant, request, resolver).await.map_err(classify)
    }

    /// Reads the answer for `id` (none for a notification) from a JSON or event-stream body.
    pub(super) async fn answer(
        &self,
        response: &mut GatewayStreamResponse,
        id: Option<&RequestId>,
        had_session: bool,
        credential: Option<&RequestCredential>,
    ) -> Result<Option<Answer>, Failure> {
        let header = |name: &str| {
            response
                .headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.clone())
        };
        if let Some(session) = header("mcp-session-id")
            && current(&self.session_id).is_none()
            && let Ok(mut slot) = self.session_id.lock()
        {
            *slot = Some(session);
        }
        let streamed = header("content-type")
            .is_some_and(|kind| kind.to_ascii_lowercase().contains("text/event-stream"));
        let status = response.status;
        match status {
            404 if had_session => return Err(Failure::Expired),
            401 => {
                self.note("[HTTP 401] 服务器要求登录或拒绝了登录凭据");
                return Err(Failure::Unauthorized);
            }
            202 if id.is_none() => return Ok(None),
            202 => return Err(Failure::Lost("服务器只表示收到，没有给出结果".into())),
            200..=299 => {}
            400..=499 => {
                self.note(&format!("[HTTP {status}] 服务器拒绝了请求"));
                let login = if status == 403 { "，需要授权" } else { "" };
                return Err(Failure::NotSent(format!("服务器拒绝了请求（HTTP {status}{login}）")));
            }
            _ => {
                self.note(&format!("[HTTP {status}] 服务器出错"));
                return Err(Failure::Lost(format!("服务器出错（HTTP {status}）")));
            }
        }
        if streamed {
            return self.read_events(response, id, credential).await;
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.body.next_chunk().await.map_err(classify)? {
            body.extend_from_slice(&chunk);
        }
        if body.iter().all(u8::is_ascii_whitespace) {
            return match id {
                None => Ok(None),
                Some(_) => Err(Failure::Lost("服务器的回应是空的".into())),
            };
        }
        let messages = parse_messages(&body).map_err(|error| Failure::Lost(error.to_string()))?;
        let mut found = None;
        for message in messages {
            if let Some(answer) = self.route(message, id, credential).await {
                found = Some(answer);
            }
        }
        match (found, id) {
            (Some(answer), _) => Ok(Some(answer)),
            (None, None) => Ok(None),
            (None, Some(_)) => Err(Failure::Lost("回应里没有这个请求的结果".into())),
        }
    }

    async fn read_events(
        &self,
        response: &mut GatewayStreamResponse,
        id: Option<&RequestId>,
        credential: Option<&RequestCredential>,
    ) -> Result<Option<Answer>, Failure> {
        let mut parser = SseParser::new(MAX_MESSAGE_BYTES);
        loop {
            let Some(chunk) = response.body.next_chunk().await.map_err(classify)? else {
                return match id {
                    None => Ok(None),
                    Some(_) => Err(Failure::Lost("回应在给出结果之前结束".into())),
                };
            };
            let events = parser.feed(&chunk).map_err(|error| Failure::Lost(error.to_string()))?;
            for event in events.iter().filter(|event| SseEvent::is_message(event)) {
                match parse_messages(event.data.as_bytes()) {
                    Ok(messages) => {
                        for message in messages {
                            if let Some(answer) = self.route(message, id, credential).await {
                                return Ok(Some(answer));
                            }
                        }
                    }
                    Err(error) => self.note(&format!("[无法解析的事件] {error}")),
                }
            }
        }
    }

    /// The answer when `message` is the one awaited; anything else is handled here.
    async fn route(
        &self,
        message: Message,
        id: Option<&RequestId>,
        credential: Option<&RequestCredential>,
    ) -> Option<Answer> {
        match message {
            Message::Response { id: answered, result } if Some(&answered) == id => {
                Some(Answer::Result(result))
            }
            Message::Error { id: Some(answered), error } if Some(&answered) == id => {
                Some(Answer::Error(error))
            }
            Message::Error { id: None, error } => {
                self.note(&format!("[服务器错误 {}] {}", error.code, error.message));
                None
            }
            Message::Request { id: asked, method, .. } => {
                let reply = if method == "ping" {
                    encode_response(&asked, json!({}))
                } else {
                    encode_error(&asked, -32601, "Aura does not offer this capability")
                };
                // A reply is a POST of its own; the server only acknowledges it.
                let _ = self.send("POST", reply, CANCEL_GRACE, credential).await;
                None
            }
            Message::Notification { method, params } => {
                if method == "notifications/tools/list_changed" {
                    self.tools_changed.store(true, Ordering::Release);
                } else if method == "notifications/message"
                    && let Some(data) = params.as_ref().and_then(|params| params.get("data"))
                {
                    self.note(&format!("[服务器日志] {data}"));
                }
                None
            }
            Message::Response { .. } | Message::Error { .. } => None,
        }
    }
}
