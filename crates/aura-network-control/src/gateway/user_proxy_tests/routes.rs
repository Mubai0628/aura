use super::proxy::{Behavior, proxy};
use super::*;

#[tokio::test]
async fn public_https_uses_http_connect_and_socks5_remote_dns() {
    for protocol in [ToolNetworkProxyProtocolV1::Http, ToolNetworkProxyProtocolV1::Socks5] {
        let target = target(true, vec![Reply::ok()]).await;
        let proxy = proxy(protocol, Behavior::Relay, &[target.address]).await;
        let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
        let response = run(
            &gateway,
            request(target.url("https", "proxy-target.test")),
            Some(proxy.settings.clone()),
            true,
        )
        .await
        .expect("HTTPS fixture");
        assert_eq!(response.body, b"local proxy fixture");
        assert_eq!(
            proxy.destinations.lock().expect("destinations").as_slice(),
            &[("proxy-target.test".into(), target.address.port(), true)],
            "hostname must reach the proxy"
        );
        assert_eq!(target.requests.lock().expect("requests").len(), 1);
    }
}

#[tokio::test]
async fn redirect_reclassifies_every_hop_and_https_to_http_goes_direct() {
    let final_target = target(false, vec![Reply::ok()]).await;
    let middle =
        target(true, vec![Reply::redirect(final_target.url("http", "proxy-target.test"))]).await;
    let first =
        target(true, vec![Reply::redirect(middle.url("https", "redirect-target.test"))]).await;
    let proxy =
        proxy(ToolNetworkProxyProtocolV1::Http, Behavior::Relay, &[first.address, middle.address])
            .await;
    let gateway =
        gateway(&[("proxy-target.test", &["8.8.8.8"]), ("redirect-target.test", &["8.8.4.4"])]);
    let response = run(
        &gateway,
        request(first.url("https", "proxy-target.test")),
        Some(proxy.settings.clone()),
        true,
    )
    .await
    .expect("redirects");
    assert_eq!(response.redirect_hops, 2);
    let destinations = proxy.destinations.lock().expect("destinations");
    assert_eq!(destinations.len(), 2);
    assert_eq!(destinations[0].0, "proxy-target.test");
    assert_eq!(destinations[1].0, "redirect-target.test");
    assert_eq!(final_target.requests.lock().expect("http direct").len(), 1);
}

#[tokio::test]
async fn http_nonpublic_mixed_answers_and_provider_channel_stay_direct() {
    for (scheme, answers, ordinary) in [
        ("http", vec!["8.8.8.8"], true),
        ("https", vec!["127.0.0.1"], true),
        ("https", vec!["10.0.0.1"], true),
        ("https", vec!["169.254.10.1"], true),
        ("https", vec!["169.254.169.254"], true),
        ("https", vec!["8.8.8.8", "10.0.0.1"], true),
        ("https", vec!["8.8.8.8"], false),
    ] {
        let target = target(scheme == "https", vec![Reply::ok()]).await;
        let proxy =
            proxy(ToolNetworkProxyProtocolV1::Http, Behavior::Relay, &[target.address]).await;
        let gateway = gateway(&[("proxy-target.test", &answers)]);
        run(
            &gateway,
            request(target.url(scheme, "proxy-target.test")),
            Some(proxy.settings.clone()),
            ordinary,
        )
        .await
        .expect("direct fixture");
        assert!(proxy.handshakes.lock().expect("proxy unused").is_empty());
        assert_eq!(target.requests.lock().expect("direct").len(), 1);
    }
}

#[tokio::test]
async fn mixed_dns_answers_still_require_the_existing_grant_classes() {
    let target = target(true, vec![Reply::ok()]).await;
    let proxy = proxy(ToolNetworkProxyProtocolV1::Http, Behavior::Relay, &[target.address]).await;
    let gateway = gateway(&[("proxy-target.test", &["8.8.8.8", "10.0.0.1"])]);
    let request = request(target.url("https", "proxy-target.test"));
    let mut grant = grant(&request, Some(proxy.settings.clone()), true);
    grant.destination_policy.allowed_destination_classes = vec![NetworkDestinationClass::Public];
    assert!(matches!(
        gateway.open_response(grant, request, None, None).await,
        Err(NetworkControlError::CoreGrantBindingMismatch)
    ));
    assert!(proxy.handshakes.lock().expect("no proxy").is_empty());
    assert!(target.requests.lock().expect("no request").is_empty());
}

#[tokio::test]
async fn registered_target_and_proxy_control_endpoints_are_refused_before_connect() {
    use crate::ordinary_tools::register_control_endpoint;
    use aura_domain_common::RuntimeInstanceId;
    let target = target(true, vec![Reply::ok()]).await;
    let proxy = proxy(ToolNetworkProxyProtocolV1::Http, Behavior::Relay, &[target.address]).await;
    let target_registration = register_control_endpoint(RuntimeInstanceId::new(), target.address)
        .expect("register target");
    let local_gateway = gateway(&[("proxy-target.test", &["127.0.0.1"])]);
    assert!(matches!(
        run(
            &local_gateway,
            request(target.url("https", "proxy-target.test")),
            Some(proxy.settings.clone()),
            true
        )
        .await,
        Err(NetworkControlError::ControlEndpointBlocked)
    ));
    drop(target_registration);
    let _proxy_registration = register_control_endpoint(
        RuntimeInstanceId::new(),
        format!("127.0.0.1:{}", proxy.settings.port).parse().expect("proxy address"),
    )
    .expect("register proxy");
    let public_gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
    assert!(matches!(
        run(
            &public_gateway,
            request(target.url("https", "proxy-target.test")),
            Some(proxy.settings.clone()),
            true
        )
        .await,
        Err(NetworkControlError::ControlEndpointBlocked)
    ));
    assert!(proxy.handshakes.lock().expect("no proxy").is_empty());
    assert!(target.requests.lock().expect("no request").is_empty());
}

#[tokio::test]
async fn disabled_proxy_and_localhost_ipv6_alias_use_the_expected_transport() {
    let target = target(true, vec![Reply::ok()]).await;
    let proxy = super::proxy::proxy_on_loopback(
        ToolNetworkProxyProtocolV1::Socks5,
        Behavior::Relay,
        &[target.address],
        "[::1]:0",
    )
    .await;
    let gateway = gateway(&[("proxy-target.test", &["8.8.8.8"])]);
    let mut disabled = proxy.settings.clone();
    disabled.enabled = false;
    run(&gateway, request(target.url("https", "proxy-target.test")), Some(disabled), true)
        .await
        .expect("disabled direct");
    assert!(proxy.destinations.lock().expect("not used").is_empty());
    let mut localhost = proxy.settings.clone();
    localhost.host = "localhost".into();
    run(&gateway, request(target.url("https", "proxy-target.test")), Some(localhost), true)
        .await
        .expect("localhost IPv6 listener");
    assert_eq!(proxy.destinations.lock().expect("used once").len(), 1);
    assert_eq!(target.requests.lock().expect("two requests").len(), 2);
}

#[test]
fn dns_fixture_cannot_connect_to_a_public_literal() {
    let record = crate::TestFixtureDnsRecord {
        host: "8.8.8.8".into(),
        classified_addresses: vec!["8.8.8.8".parse().expect("public")],
        connect_addresses: vec!["127.0.0.1".parse().expect("loopback")],
    };
    assert!(NetworkControlOptions::default().with_test_fixture_dns(vec![record]).is_err());
}
