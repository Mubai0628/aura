//! A name the system's TUN proxy answers with one of its own addresses is reached as a public one,
//! so the connection goes to the TUN and the proxy's own rules choose the way out. A local name
//! answered the same way keeps its reserved class.
use super::*;

#[tokio::test]
async fn a_public_name_the_tun_proxy_answers_is_reached_as_public() {
    let target = target(false, vec![Reply::ok()]).await;
    let gateway = gateway(&[("api.tun-route.example", &["198.18.3.32"])]);
    let response = run(&gateway, request(target.url("http", "api.tun-route.example")), None, false)
        .await
        .expect("reached through the system's route");
    assert_eq!(response.body, b"local proxy fixture");
    assert_eq!(response.final_destination.classes, vec![NetworkDestinationClass::Public]);
    assert_eq!(response.final_destination.addresses, vec!["198.18.3.32".to_owned()]);
    assert_eq!(target.requests.lock().expect("requests").len(), 1);
}

#[tokio::test]
async fn a_local_name_the_tun_proxy_answers_keeps_its_class() {
    let target = target(false, vec![Reply::ok()]).await;
    let gateway = gateway(&[("nas.lan", &["198.18.0.5"])]);
    let response = run(&gateway, request(target.url("http", "nas.lan")), None, false)
        .await
        .expect("this fixture grant allows every class");
    assert_eq!(response.final_destination.classes, vec![NetworkDestinationClass::Reserved]);
}

#[tokio::test]
async fn a_public_name_the_tun_proxy_answers_needs_no_private_network_approval() {
    let gateway =
        gateway(&[("api.tun-route.example", &["198.18.3.32"]), ("nas.lan", &["198.18.0.5"])]);
    let plan = |url: &str| request(url.to_owned()).plan;
    gateway
        .authorize(plan("https://api.tun-route.example/"))
        .await
        .expect("a public name through the TUN");
    assert!(matches!(
        gateway.authorize(plan("https://nas.lan/")).await,
        Err(NetworkControlError::PrivateNetworkApprovalRequired)
    ));
}
