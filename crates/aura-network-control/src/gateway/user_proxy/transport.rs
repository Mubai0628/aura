use super::{HopSendError, UserProxyFailureKindV1 as Kind, failure, handshake, tls};
use crate::{GatewayRequest, NetworkControlError, NetworkControlOptions};
use aura_tool_contract::ToolNetworkProxyV1;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::{TokioExecutor, TokioIo};
use reqwest::{
    Method,
    header::{ACCEPT, HOST, HeaderMap, HeaderValue},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::{Instant, timeout_at},
};
use tokio_rustls::rustls::pki_types::ServerName;
use url::Url;

#[derive(Clone)]
pub(in crate::gateway) struct ProxyClient {
    proxy: ToolNetworkProxyV1,
    tls: tokio_rustls::TlsConnector,
}

/// What one proxied hop sends, and how long it may take: `connect` to reach the target through
/// the proxy, `deadline` for the whole exchange, body included.
pub(in crate::gateway) struct ProxiedHop<'a> {
    pub url: &'a str,
    pub method: Method,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub connect: Duration,
    pub deadline: Instant,
}

impl ProxyClient {
    pub(in crate::gateway) fn new(
        proxy: ToolNetworkProxyV1,
        options: &NetworkControlOptions,
    ) -> Result<Self, NetworkControlError> {
        Ok(Self { proxy, tls: tls::connector(options)? })
    }

    pub(in crate::gateway) async fn send(
        &self,
        request: &GatewayRequest,
        method: Method,
        headers: HeaderMap,
    ) -> Result<reqwest::Response, HopSendError> {
        // Reserve time for the owner's direct recovery. A Windows refused TCP connect can
        // itself wait for a SYN timeout; it must not consume the complete hop budget.
        self.send_hop(ProxiedHop {
            url: &request.plan.requested_url,
            method,
            headers,
            body: request.body.clone(),
            connect: Duration::from_millis((request.plan.timeout_ms / 2).clamp(1, 5_000)),
            deadline: Instant::now() + Duration::from_millis(request.plan.timeout_ms),
        })
        .await
    }

    pub(in crate::gateway) async fn send_hop(
        &self,
        hop: ProxiedHop<'_>,
    ) -> Result<reqwest::Response, HopSendError> {
        let started = Arc::new(AtomicBool::new(false));
        let deadline = hop.deadline;
        match timeout_at(deadline, self.exchange(hop, &started)).await {
            Ok(result) => result.map_err(|error| {
                let request_not_started = !started.load(Ordering::SeqCst);
                let error =
                    if request_not_started && matches!(error, NetworkControlError::Transport(_)) {
                        failure(&self.proxy, Kind::Protocol, "代理隧道后的 HTTP 连接未能建立")
                    } else {
                        error
                    };
                HopSendError { error, request_not_started }
            }),
            Err(_) => Err(HopSendError {
                error: if started.load(Ordering::SeqCst) {
                    NetworkControlError::Timeout
                } else {
                    failure(&self.proxy, Kind::Unavailable, "连接代理或目标 TLS 握手超时")
                },
                request_not_started: !started.load(Ordering::SeqCst),
            }),
        }
    }

    async fn exchange(
        &self,
        hop: ProxiedHop<'_>,
        started: &AtomicBool,
    ) -> Result<reqwest::Response, NetworkControlError> {
        let ProxiedHop { url, method, headers, body, connect: connect_budget, deadline } = hop;
        let url = Url::parse(url).map_err(|_| NetworkControlError::InvalidUrl)?;
        let host = url.host_str().ok_or(NetworkControlError::InvalidUrl)?.trim_matches(['[', ']']);
        let port = url.port_or_known_default().ok_or(NetworkControlError::InvalidUrl)?;
        let server_name =
            ServerName::try_from(host.to_owned()).map_err(|_| NetworkControlError::InvalidUrl)?;
        let establish = async {
            let socket = handshake::connect(&self.proxy, host, port).await?;
            self.tls.connect(server_name, socket).await.map_err(|_| {
                failure(&self.proxy, Kind::Unavailable, "经代理的目标 TLS 握手或证书校验失败")
            })
        };
        let tls = tokio::time::timeout(connect_budget, establish).await.map_err(|_| {
            failure(&self.proxy, Kind::Unavailable, "连接代理或目标 TLS 握手超时")
        })??;
        let h2 = tls.get_ref().1.alpn_protocol() == Some(b"h2");
        let response =
            send_http(TokioIo::new(tls), &url, method, headers, body, h2, started, deadline)
                .await?;
        let (parts, incoming) = response.into_parts();
        // Preserve streaming, wire/decompressed limits and the hop's total deadline.
        let stream = futures_util::stream::unfold(
            (incoming, deadline, false),
            |(mut body, deadline, ended)| async move {
                if ended {
                    return None;
                }
                loop {
                    match timeout_at(deadline, body.frame()).await {
                        Ok(Some(Ok(frame))) => {
                            if let Ok(bytes) = frame.into_data() {
                                return Some((Ok(bytes), (body, deadline, false)));
                            }
                        }
                        Ok(None) => return None,
                        Ok(Some(Err(_))) => {
                            return Some((
                                Err(std::io::Error::from(std::io::ErrorKind::ConnectionAborted)),
                                (body, deadline, true),
                            ));
                        }
                        Err(_) => {
                            return Some((
                                Err(std::io::Error::from(std::io::ErrorKind::TimedOut)),
                                (body, deadline, true),
                            ));
                        }
                    }
                }
            },
        );
        Ok(reqwest::Response::from(hyper::Response::from_parts(
            parts,
            reqwest::Body::wrap_stream(stream),
        )))
    }
}

#[allow(clippy::too_many_arguments)]
async fn send_http<T>(
    io: TokioIo<T>,
    url: &Url,
    method: Method,
    mut headers: HeaderMap,
    body: Vec<u8>,
    h2: bool,
    started: &AtomicBool,
    deadline: Instant,
) -> Result<hyper::Response<hyper::body::Incoming>, NetworkControlError>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let uri = if h2 {
        url.as_str().to_owned()
    } else {
        let query = url.query().map(|query| format!("?{query}")).unwrap_or_default();
        format!("{}{query}", url.path())
    };
    headers.entry(ACCEPT).or_insert(HeaderValue::from_static("*/*"));
    if h2 {
        // HTTP/2 names the target only by `:authority`, taken from the absolute URI. Strict
        // servers, Google among them, reset a stream that also carries Host (PROTOCOL_ERROR).
        headers.remove(HOST);
    } else if !headers.contains_key(HOST) {
        let host = url.host_str().ok_or(NetworkControlError::InvalidUrl)?;
        let authority = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.into(),
        };
        headers.insert(HOST, authority.parse().map_err(|_| NetworkControlError::InvalidUrl)?);
    }
    let mut request = hyper::Request::builder()
        .method(method)
        .uri(uri)
        .body(Full::new(Bytes::from(body)))
        .map_err(|_| NetworkControlError::InvalidUrl)?;
    *request.headers_mut() = headers;
    if h2 {
        let (mut sender, connection) =
            hyper::client::conn::http2::handshake(TokioExecutor::new(), io).await.map_err(
                |_| {
                    NetworkControlError::Transport("HTTP/2 connection failed before request".into())
                },
            )?;
        tokio::spawn(async move {
            let _ = timeout_at(deadline, connection).await;
        });
        sender
            .ready()
            .await
            .map_err(|_| NetworkControlError::Transport("HTTP/2 connection not ready".into()))?;
        started.store(true, Ordering::SeqCst);
        sender
            .send_request(request)
            .await
            .map_err(|_| NetworkControlError::Transport("request transport failed".into()))
    } else {
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(io).await.map_err(|_| {
                NetworkControlError::Transport("HTTP connection failed before request".into())
            })?;
        tokio::spawn(async move {
            let _ = timeout_at(deadline, connection).await;
        });
        sender
            .ready()
            .await
            .map_err(|_| NetworkControlError::Transport("HTTP connection not ready".into()))?;
        started.store(true, Ordering::SeqCst);
        sender
            .send_request(request)
            .await
            .map_err(|_| NetworkControlError::Transport("request transport failed".into()))
    }
}
