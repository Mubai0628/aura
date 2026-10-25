#![forbid(unsafe_code)]

use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use thiserror::Error;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::Notify,
    task::JoinHandle,
};

const MAX_REQUEST_HEAD_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureResponseChunk {
    pub delay: Duration,
    pub bytes: Vec<u8>,
}

impl FixtureResponseChunk {
    #[must_use]
    pub fn immediate(bytes: impl Into<Vec<u8>>) -> Self {
        Self { delay: Duration::ZERO, bytes: bytes.into() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub chunks: Vec<FixtureResponseChunk>,
    pub termination: FixtureResponseTermination,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureResponseTermination {
    Complete,
    Disconnect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelProviderScenario {
    OpenAiResponsesReasoningTool,
    OpenAiResponsesRefusal,
    OpenAiChatParallelTools,
    OpenAiChatRefusal,
    AnthropicThinkingTool,
    AnthropicRefusal,
    DiscoveryModels,
    AuthenticationFailed,
    ContextWindowExceeded,
    RateLimited,
    QuotaExhausted,
    ServiceUnavailable,
    UnsupportedOptionalParameter,
}

impl ModelProviderScenario {
    #[must_use]
    pub const fn payload(self) -> &'static [u8] {
        match self {
            Self::OpenAiResponsesReasoningTool => include_bytes!(
                "../../../fixtures/model-providers/openai-responses/reasoning-tool.sse"
            ),
            Self::OpenAiResponsesRefusal => {
                include_bytes!("../../../fixtures/model-providers/openai-responses/refusal.sse")
            }
            Self::OpenAiChatParallelTools => {
                include_bytes!("../../../fixtures/model-providers/openai-chat/parallel-tools.sse")
            }
            Self::OpenAiChatRefusal => {
                include_bytes!("../../../fixtures/model-providers/openai-chat/refusal.sse")
            }
            Self::AnthropicThinkingTool => {
                include_bytes!("../../../fixtures/model-providers/anthropic/thinking-tool.sse")
            }
            Self::AnthropicRefusal => {
                include_bytes!("../../../fixtures/model-providers/anthropic/refusal.sse")
            }
            Self::DiscoveryModels => {
                include_bytes!("../../../fixtures/model-providers/discovery/models.json")
            }
            Self::AuthenticationFailed => {
                include_bytes!(
                    "../../../fixtures/model-providers/errors/authentication-failed.json"
                )
            }
            Self::ContextWindowExceeded => include_bytes!(
                "../../../fixtures/model-providers/errors/context-window-exceeded.json"
            ),
            Self::RateLimited => {
                include_bytes!("../../../fixtures/model-providers/errors/rate-limited.json")
            }
            Self::QuotaExhausted => {
                include_bytes!("../../../fixtures/model-providers/errors/quota-exhausted.json")
            }
            Self::ServiceUnavailable => {
                include_bytes!("../../../fixtures/model-providers/errors/service-unavailable.json")
            }
            Self::UnsupportedOptionalParameter => include_bytes!(
                "../../../fixtures/model-providers/compatibility/unsupported-optional-parameter.json"
            ),
        }
    }

    #[must_use]
    pub fn response(self) -> FixtureResponse {
        match self {
            Self::OpenAiResponsesReasoningTool
            | Self::OpenAiResponsesRefusal
            | Self::OpenAiChatParallelTools
            | Self::OpenAiChatRefusal
            | Self::AnthropicThinkingTool
            | Self::AnthropicRefusal => {
                FixtureResponse::sse([FixtureResponseChunk::immediate(self.payload().to_vec())])
            }
            Self::DiscoveryModels => FixtureResponse::json(200, self.payload().to_vec()),
            Self::AuthenticationFailed => FixtureResponse::json_error(401, self.payload().to_vec()),
            Self::ContextWindowExceeded | Self::UnsupportedOptionalParameter => {
                FixtureResponse::json_error(400, self.payload().to_vec())
            }
            Self::RateLimited => {
                let mut response = FixtureResponse::json_error(429, self.payload().to_vec());
                response.headers.push(("retry-after".to_owned(), "1".to_owned()));
                response
            }
            Self::QuotaExhausted => FixtureResponse::json_error(429, self.payload().to_vec()),
            Self::ServiceUnavailable => FixtureResponse::json_error(503, self.payload().to_vec()),
        }
    }
}

impl FixtureResponse {
    #[must_use]
    pub fn sse(chunks: impl IntoIterator<Item = FixtureResponseChunk>) -> Self {
        Self {
            status: 200,
            headers: vec![("content-type".to_owned(), "text/event-stream".to_owned())],
            chunks: chunks.into_iter().collect(),
            termination: FixtureResponseTermination::Complete,
        }
    }

    #[must_use]
    pub fn json_error(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self::json(status, body)
    }

    #[must_use]
    pub fn json(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            chunks: vec![FixtureResponseChunk::immediate(body)],
            termination: FixtureResponseTermination::Complete,
        }
    }

    #[must_use]
    pub fn disconnect_after_chunks(mut self) -> Self {
        self.termination = FixtureResponseTermination::Disconnect;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedFixtureRequest {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum FixtureServerError {
    #[error("fixture server I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("fixture request exceeded the configured head limit")]
    RequestHeadTooLarge,
    #[error("fixture request exceeded the configured body limit")]
    RequestBodyTooLarge,
    #[error("fixture request was malformed: {0}")]
    MalformedRequest(&'static str),
    #[error("fixture server request wait timed out")]
    WaitTimedOut,
    #[error("fixture server state lock was poisoned")]
    StatePoisoned,
}

struct FixtureState {
    responses: VecDeque<FixtureResponse>,
    requests: Vec<CapturedFixtureRequest>,
}

pub struct FixtureServer {
    address: SocketAddr,
    state: Arc<Mutex<FixtureState>>,
    changed: Arc<Notify>,
    task: JoinHandle<Result<(), FixtureServerError>>,
}

impl FixtureServer {
    pub async fn start(
        responses: impl IntoIterator<Item = FixtureResponse>,
    ) -> Result<Self, FixtureServerError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let address = listener.local_addr()?;
        let state = Arc::new(Mutex::new(FixtureState {
            responses: responses.into_iter().collect(),
            requests: Vec::new(),
        }));
        let changed = Arc::new(Notify::new());
        let task_state = Arc::clone(&state);
        let task_changed = Arc::clone(&changed);
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await?;
                let request = read_request(&mut stream).await?;
                let response = {
                    let mut guard =
                        task_state.lock().map_err(|_| FixtureServerError::StatePoisoned)?;
                    guard.requests.push(request);
                    guard.responses.pop_front()
                };
                task_changed.notify_waiters();
                let response = response.unwrap_or_else(|| {
                    FixtureResponse::json_error(
                        500,
                        br#"{"error":"fixture response script exhausted"}"#.to_vec(),
                    )
                });
                write_response(&mut stream, response).await?;
            }
        });
        Ok(Self { address, state, changed, task })
    }

    #[must_use]
    pub fn origin(&self) -> String {
        format!("http://{}", self.address)
    }

    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn requests(&self) -> Result<Vec<CapturedFixtureRequest>, FixtureServerError> {
        self.state
            .lock()
            .map_err(|_| FixtureServerError::StatePoisoned)
            .map(|state| state.requests.clone())
    }

    pub async fn wait_for_requests(
        &self,
        expected: usize,
        timeout: Duration,
    ) -> Result<Vec<CapturedFixtureRequest>, FixtureServerError> {
        tokio::time::timeout(timeout, async {
            loop {
                let requests = self.requests()?;
                if requests.len() >= expected {
                    return Ok(requests);
                }
                self.changed.notified().await;
            }
        })
        .await
        .map_err(|_| FixtureServerError::WaitTimedOut)?
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read_request(
    stream: &mut TcpStream,
) -> Result<CapturedFixtureRequest, FixtureServerError> {
    let mut bytes = Vec::new();
    let header_end = loop {
        if bytes.len() > MAX_REQUEST_HEAD_BYTES {
            return Err(FixtureServerError::RequestHeadTooLarge);
        }
        if let Some(index) = find_bytes(&bytes, b"\r\n\r\n") {
            break index + 4;
        }
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(FixtureServerError::MalformedRequest("request head ended early"));
        }
        bytes.extend_from_slice(&buffer[..read]);
    };
    let head = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| FixtureServerError::MalformedRequest("request head is not UTF-8"))?;
    let mut lines = head[..head.len() - 4].split("\r\n");
    let request_line =
        lines.next().ok_or(FixtureServerError::MalformedRequest("request line is missing"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or(FixtureServerError::MalformedRequest("request method is missing"))?
        .to_owned();
    let target = request_parts
        .next()
        .ok_or(FixtureServerError::MalformedRequest("request target is missing"))?
        .to_owned();
    if request_parts.next() != Some("HTTP/1.1") || request_parts.next().is_some() {
        return Err(FixtureServerError::MalformedRequest("request line is invalid"));
    }
    let mut headers = Vec::new();
    let mut content_length = 0_usize;
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or(FixtureServerError::MalformedRequest("request header is invalid"))?;
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_owned();
        if name == "content-length" {
            content_length = value
                .parse::<usize>()
                .map_err(|_| FixtureServerError::MalformedRequest("content-length is invalid"))?;
        }
        headers.push((name, value));
    }
    if content_length > MAX_REQUEST_BODY_BYTES {
        return Err(FixtureServerError::RequestBodyTooLarge);
    }
    while bytes.len() - header_end < content_length {
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(FixtureServerError::MalformedRequest("request body ended early"));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() - header_end > MAX_REQUEST_BODY_BYTES {
            return Err(FixtureServerError::RequestBodyTooLarge);
        }
    }
    let body = bytes[header_end..header_end + content_length].to_vec();
    Ok(CapturedFixtureRequest { method, target, headers, body })
}

async fn write_response(
    stream: &mut TcpStream,
    response: FixtureResponse,
) -> Result<(), FixtureServerError> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", response.status, reason(response.status));
    for (name, value) in response.headers {
        if name.eq_ignore_ascii_case("content-length")
            || name.eq_ignore_ascii_case("transfer-encoding")
            || name.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        head.push_str(&name);
        head.push_str(": ");
        head.push_str(&value);
        head.push_str("\r\n");
    }
    head.push_str("transfer-encoding: chunked\r\nconnection: close\r\n\r\n");
    stream.write_all(head.as_bytes()).await?;
    for chunk in response.chunks {
        if !chunk.delay.is_zero() {
            tokio::time::sleep(chunk.delay).await;
        }
        if chunk.bytes.is_empty() {
            continue;
        }
        stream.write_all(format!("{:X}\r\n", chunk.bytes.len()).as_bytes()).await?;
        stream.write_all(&chunk.bytes).await?;
        stream.write_all(b"\r\n").await?;
        stream.flush().await?;
    }
    if response.termination == FixtureResponseTermination::Complete {
        stream.write_all(b"0\r\n\r\n").await?;
    }
    stream.shutdown().await?;
    Ok(())
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

const fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        408 => "Request Timeout",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        529 => "Overloaded",
        _ => "Fixture Response",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn serves_chunked_sse_and_captures_request() {
        let server = FixtureServer::start([FixtureResponse::sse([
            FixtureResponseChunk::immediate(b"data: one\n\n".to_vec()),
            FixtureResponseChunk::immediate(b"data: two\n\n".to_vec()),
        ])])
        .await
        .expect("fixture server");
        let mut stream = TcpStream::connect(server.address()).await.expect("connect");
        stream
            .write_all(b"POST /v1/test HTTP/1.1\r\nhost: fixture\r\ncontent-length: 2\r\n\r\n{}")
            .await
            .expect("write request");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read response");
        assert!(response.windows(b"data: one".len()).any(|part| part == b"data: one"));
        assert!(response.windows(b"data: two".len()).any(|part| part == b"data: two"));
        let requests =
            server.wait_for_requests(1, Duration::from_secs(1)).await.expect("captured request");
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].target, "/v1/test");
        assert_eq!(requests[0].body, b"{}");
    }

    #[tokio::test]
    async fn can_disconnect_without_completing_chunked_transfer() {
        let server =
            FixtureServer::start([FixtureResponse::sse([FixtureResponseChunk::immediate(
                b"data: partial\n\n".to_vec(),
            )])
            .disconnect_after_chunks()])
            .await
            .expect("fixture server");
        let mut stream = TcpStream::connect(server.address()).await.expect("connect");
        stream
            .write_all(b"POST /v1/test HTTP/1.1\r\nhost: fixture\r\ncontent-length: 2\r\n\r\n{}")
            .await
            .expect("write request");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read response");
        assert!(response.windows(b"data: partial".len()).any(|part| part == b"data: partial"));
        assert!(!response.ends_with(b"0\r\n\r\n"));
    }

    #[test]
    fn exposes_reviewed_model_provider_scenarios() {
        let streaming = ModelProviderScenario::OpenAiResponsesReasoningTool.response();
        assert_eq!(streaming.status, 200);
        assert!(
            streaming.chunks[0]
                .bytes
                .windows(b"response.completed".len())
                .any(|part| { part == b"response.completed" })
        );

        for scenario in [
            ModelProviderScenario::OpenAiResponsesRefusal,
            ModelProviderScenario::OpenAiChatRefusal,
            ModelProviderScenario::AnthropicRefusal,
        ] {
            let refusal = scenario.response();
            assert_eq!(refusal.status, 200);
            assert!(
                refusal.chunks[0].bytes.windows(b"refusal".len()).any(|part| part == b"refusal")
            );
        }

        let limited = ModelProviderScenario::RateLimited.response();
        assert_eq!(limited.status, 429);
        assert!(
            limited.headers.iter().any(|(name, value)| { name == "retry-after" && value == "1" })
        );

        let authentication = ModelProviderScenario::AuthenticationFailed.response();
        assert_eq!(authentication.status, 401);

        let unavailable = ModelProviderScenario::ServiceUnavailable.response();
        assert_eq!(unavailable.status, 503);

        let discovery = ModelProviderScenario::DiscoveryModels.response();
        assert_eq!(discovery.status, 200);
        assert_eq!(discovery.headers[0].1, "application/json");
    }
}
