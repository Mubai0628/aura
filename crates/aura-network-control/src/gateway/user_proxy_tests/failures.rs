use super::proxy::{Behavior, proxy};
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn connect_refused_behaves_as_disabled_and_records_the_proxy_failure() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("unused port");
    let port = listener.local_addr().expect("port").port();
    drop(listener);
    let proxy =
        ToolNetworkProxyV1::new(true, ToolNetworkProxyProtocolV1::Http, "127.0.0.1".into(), port)
            .expect("proxy");
    let target = target(true, vec![Reply::ok()]).await;
    let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
    let request = request(target.url("https", "proxy-target.test"));
    let opened = gateway
        .open_response(grant(&request, Some(proxy), true), request, None, None)
        .await
        .expect("direct recovery");
    assert_eq!(opened.routing.len(), 1);
    assert_eq!(opened.routing[0].route, GatewayRouteV1::Direct);
    assert_eq!(
        opened.routing[0].proxy_failure.as_ref().expect("reason").kind,
        UserProxyFailureKindV1::Unavailable
    );
    gateway.collect_response(opened).await.expect("body");
    assert_eq!(target.requests.lock().expect("request").len(), 1);
}

#[tokio::test]
async fn connect_and_socks_refusals_preserve_the_actual_reason_before_direct_recovery() {
    for protocol in [ToolNetworkProxyProtocolV1::Http, ToolNetworkProxyProtocolV1::Socks5] {
        let target = target(true, vec![Reply::ok()]).await;
        let proxy = proxy(protocol, Behavior::Refuse, &[target.address]).await;
        let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
        let request = request(target.url("https", "proxy-target.test"));
        let opened = gateway
            .open_response(grant(&request, Some(proxy.settings.clone()), true), request, None, None)
            .await
            .expect("direct recovery");
        let failed = opened.routing[0].proxy_failure.as_ref().expect("proxy reason");
        assert_eq!(failed.kind, UserProxyFailureKindV1::Refused);
        if protocol == ToolNetworkProxyProtocolV1::Http {
            assert!(failed.reason.contains("403 Fixture policy"));
        } else {
            assert!(failed.reason.contains("2：代理规则拒绝"));
        }
        assert!(proxy.destinations.lock().expect("no tunnel").is_empty());
        gateway.collect_response(opened).await.expect("body");
        assert_eq!(target.requests.lock().expect("only direct").len(), 1);
    }
}

#[tokio::test]
async fn protocol_mismatch_is_explained_and_does_not_disable_network_tools() {
    for (actual, configured) in [
        (ToolNetworkProxyProtocolV1::Http, ToolNetworkProxyProtocolV1::Socks5),
        (ToolNetworkProxyProtocolV1::Socks5, ToolNetworkProxyProtocolV1::Http),
    ] {
        let target = target(true, vec![Reply::ok()]).await;
        let proxy = proxy(actual, Behavior::Relay, &[target.address]).await;
        let mut settings = proxy.settings.clone();
        settings.protocol = configured;
        let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
        let mut request = request(target.url("https", "proxy-target.test"));
        request.plan.timeout_ms = 500;
        let opened = gateway
            .open_response(grant(&request, Some(settings), true), request, None, None)
            .await
            .expect("direct recovery");
        let failed = opened.routing[0].proxy_failure.as_ref().expect("reason");
        assert_eq!(failed.kind, UserProxyFailureKindV1::Protocol);
        assert!(failed.to_string().contains("10808"));
        gateway.collect_response(opened).await.expect("body");
        assert_eq!(target.requests.lock().expect("only direct").len(), 1);
    }
}

#[tokio::test]
async fn proxy_handshake_timeout_leaves_time_for_direct_recovery() {
    let target = target(true, vec![Reply::ok()]).await;
    let proxy = proxy(ToolNetworkProxyProtocolV1::Socks5, Behavior::Stall, &[target.address]).await;
    let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
    let mut request = request(target.url("https", "proxy-target.test"));
    request.plan.timeout_ms = 100;
    let opened = gateway
        .open_response(grant(&request, Some(proxy.settings.clone()), true), request, None, None)
        .await
        .expect("proxy timeout must leave a direct recovery budget");
    let routing = opened.routing.clone();
    assert_eq!(
        routing[0].proxy_failure.as_ref().expect("timeout reason").kind,
        UserProxyFailureKindV1::Unavailable
    );
    assert_eq!(routing[0].route, GatewayRouteV1::Direct);
    gateway.collect_response(opened).await.expect("direct response");
    assert_eq!(target.requests.lock().expect("only direct HTTP").len(), 1);
}

#[tokio::test]
async fn target_certificate_name_is_checked_and_no_http_bytes_leak_on_failure() {
    let target = target(true, vec![Reply::ok()]).await;
    let proxy = proxy(ToolNetworkProxyProtocolV1::Http, Behavior::Relay, &[target.address]).await;
    let gateway = gateway(&[("uncovered-target.test", &["8.8.8.8"])]);
    let request = request(target.url("https", "uncovered-target.test"));
    let error = match gateway
        .open_response(grant(&request, Some(proxy.settings.clone()), true), request, None, None)
        .await
    {
        Ok(_) => panic!("wrong certificate name"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        NetworkControlError::RoutedTransport { request_not_started: true, .. }
    ));
    assert_eq!(proxy.destinations.lock().expect("one tunnel").len(), 1);
    assert!(target.requests.lock().expect("no HTTP").is_empty());
}

#[tokio::test]
async fn once_a_post_might_have_been_sent_it_never_replays_directly() {
    let target = target(true, vec![Reply { interrupt: true, ..Reply::ok() }]).await;
    let proxy = proxy(ToolNetworkProxyProtocolV1::Http, Behavior::Relay, &[target.address]).await;
    let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
    let mut request = request(target.url("https", "proxy-target.test"));
    request.plan.method = "POST".into();
    request.body = b"unique local POST".to_vec();
    let starts = Arc::new(AtomicUsize::new(0));
    let observed = starts.clone();
    let hook = Box::new(move || -> GatewayStartedHook {
        observed.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    });
    let error = match gateway
        .open_response(
            grant(&request, Some(proxy.settings.clone()), true),
            request,
            Some(hook),
            None,
        )
        .await
    {
        Ok(_) => panic!("interrupted POST"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        NetworkControlError::RoutedTransport { request_not_started: false, .. }
    ));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(proxy.destinations.lock().expect("only one tunnel").len(), 1);
    assert_eq!(target.requests.lock().expect("only one POST").len(), 1);
}
