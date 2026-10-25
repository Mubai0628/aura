use super::proxy::{Behavior, proxy};
use super::*;
use http_body_util::Full;
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::{io::Write, sync::Mutex};

async fn h2_target() -> Target {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("h2 listener");
    let address = listener.local_addr().expect("address");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let acceptor = tls_acceptor(vec![b"h2".to_vec()]);
    let task = tokio::spawn(async move {
        let mut workers = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (socket, _) = match accepted { Ok(v) => v, Err(_) => break };
                    let acceptor = acceptor.clone(); let observed = observed.clone();
                    workers.spawn(async move {
                        let Ok(tls) = acceptor.accept(socket).await else { return };
                        let service = hyper::service::service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                            let observed = observed.clone();
                            async move {
                                let host = request.headers().get(hyper::header::HOST).map(|value| value.to_str().unwrap_or("<binary>").to_owned());
                                observed.lock().expect("HTTP/2 requests").push(format!("{} {} host={host:?}", request.method(), request.uri()).into_bytes());
                                Ok::<_, std::convert::Infallible>(hyper::Response::new(Full::new(Bytes::from_static(b"local HTTP/2 fixture"))))
                            }
                        });
                        let connection = hyper::server::conn::http2::Builder::new(TokioExecutor::new()).serve_connection(TokioIo::new(tls), service);
                        let _ = tokio::time::timeout(Duration::from_secs(5), connection).await;
                    });
                }
                _ = workers.join_next(), if !workers.is_empty() => {}
            }
        }
    });
    Target { address, requests, task }
}

#[tokio::test]
async fn http2_alpn_works_through_both_proxy_protocols() {
    for protocol in [ToolNetworkProxyProtocolV1::Http, ToolNetworkProxyProtocolV1::Socks5] {
        let target = h2_target().await;
        let proxy = proxy(protocol, Behavior::Relay, &[target.address]).await;
        let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
        let response = run(
            &gateway,
            request(target.url("https", "proxy-target.test")),
            Some(proxy.settings.clone()),
            true,
        )
        .await
        .expect("proxied h2");
        assert_eq!(response.body, b"local HTTP/2 fixture");
        assert_eq!(proxy.destinations.lock().expect("proxy used").len(), 1);
        assert_eq!(target.requests.lock().expect("request").len(), 1);
    }
}

/// HTTP/2 names the target only by `:authority`. Google resets a stream that also carries a
/// Host header (PROTOCOL_ERROR), which made web.fetch fail through the owner's SOCKS5 proxy
/// while the direct route worked.
#[tokio::test]
async fn http2_requests_name_the_target_by_authority_without_a_host_header() {
    for protocol in [ToolNetworkProxyProtocolV1::Http, ToolNetworkProxyProtocolV1::Socks5] {
        let target = h2_target().await;
        let proxy = proxy(protocol, Behavior::Relay, &[target.address]).await;
        let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
        run(
            &gateway,
            request(target.url("https", "proxy-target.test")),
            Some(proxy.settings.clone()),
            true,
        )
        .await
        .expect("proxied h2");
        let requests = target.requests.lock().expect("request").clone();
        let requests: Vec<String> = requests
            .into_iter()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .collect();
        assert_eq!(
            requests,
            vec![format!("GET {} host=None", target.url("https", "proxy-target.test"))],
            "{protocol:?}"
        );
    }
}

#[tokio::test]
async fn proxied_streaming_preserves_wire_and_decompressed_limits() {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&vec![b'x'; 1024]).expect("compress");
    let encoded = encoder.finish().expect("gzip");
    for limit in [2048, 16] {
        let target = target(
            true,
            vec![Reply {
                body: encoded.clone(),
                headers: vec![("Content-Encoding".into(), "gzip".into())],
                ..Reply::ok()
            }],
        )
        .await;
        let proxy =
            proxy(ToolNetworkProxyProtocolV1::Socks5, Behavior::Relay, &[target.address]).await;
        let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
        let mut request = request(target.url("https", "proxy-target.test"));
        request.plan.byte_limits.max_decompressed_bytes = limit;
        let opened = gateway
            .open_response(grant(&request, Some(proxy.settings.clone()), true), request, None, None)
            .await
            .expect("headers");
        assert!(matches!(opened.routing[0].route, GatewayRouteV1::UserProxy { .. }));
        let mut body = GatewayBodyStream::new(
            opened.response,
            &opened.headers,
            opened.rate_permit,
            opened.max_download_bytes,
            opened.max_decompressed_bytes,
        )
        .expect("stream");
        if limit == 2048 {
            let mut collected = Vec::new();
            while let Some(chunk) = body.next_chunk().await.expect("chunk") {
                collected.extend(chunk);
            }
            assert_eq!(collected, vec![b'x'; 1024]);
        } else {
            assert!(matches!(body.next_chunk().await, Err(NetworkControlError::ResponseTooLarge)));
        }
        assert_eq!(target.requests.lock().expect("only one request").len(), 1);
    }
}
