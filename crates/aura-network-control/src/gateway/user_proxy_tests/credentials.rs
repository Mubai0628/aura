use super::proxy::{Behavior, proxy};
use super::*;
use aura_domain_common::{CredentialId, CredentialRevision};
use aura_permission_domain::{ToolCredentialPlacementV1, ToolCredentialUseV1};

const CANARY: &[u8] = b"Bearer l01-public-synthetic-credential";
struct FixtureCredential {
    binding: ToolCredentialUseV1,
    origin: NormalizedOrigin,
}
impl crate::ToolCredentialResolver for FixtureCredential {
    fn resolve(
        &self,
        binding: &ToolCredentialUseV1,
    ) -> Result<crate::ResolvedToolCredential, NetworkControlError> {
        if binding != &self.binding {
            return Err(NetworkControlError::CredentialBindingMismatch);
        }
        Ok(crate::ResolvedToolCredential {
            binding: crate::ToolCredentialBinding {
                credential_id: binding.credential_id,
                credential_revision: binding.credential_revision,
                origin: self.origin.clone(),
                header_name: "authorization".into(),
            },
            secret_header_value: CANARY.to_vec(),
        })
    }
}
fn credential(host: &str, port: u16) -> FixtureCredential {
    FixtureCredential {
        binding: ToolCredentialUseV1 {
            credential_id: CredentialId::new(),
            credential_revision: CredentialRevision::INITIAL,
            slot_key: aura_tool_contract::CredentialSlotKey::new("l01_fixture").expect("slot"),
            placement: ToolCredentialPlacementV1::BearerAuthorization,
        },
        origin: NormalizedOrigin { scheme: "https".into(), host: host.into(), port },
    }
}
fn credential_grant(
    request: &GatewayRequest,
    proxy: ToolNetworkProxyV1,
    credential: &FixtureCredential,
) -> CoreRequestGrant<FixturePurpose> {
    let mut grant = grant(request, Some(proxy), true);
    grant.purpose.credential = Some(credential.binding.clone());
    grant.purpose.hash =
        crate::ordinary_tools::ordinary_request_hash(request, Some(&credential.binding))
            .expect("credential-bound hash");
    grant
}
fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

#[tokio::test]
async fn credentials_and_body_exist_only_inside_target_tls_for_both_proxy_protocols() {
    for protocol in [ToolNetworkProxyProtocolV1::Http, ToolNetworkProxyProtocolV1::Socks5] {
        let target = target(true, vec![Reply::ok()]).await;
        let proxy = proxy(protocol, Behavior::Relay, &[target.address]).await;
        let gateway = gateway(&[("credential-target.test", &["8.8.8.8"])]);
        let resolver = credential("credential-target.test", target.address.port());
        let mut request = request(target.url("https", "credential-target.test"));
        request.plan.method = "POST".into();
        request.body = b"l01-private-body-canary".to_vec();
        let opened = gateway
            .open_response(
                credential_grant(&request, proxy.settings.clone(), &resolver),
                request,
                None,
                Some(&resolver),
            )
            .await
            .expect("credential HTTPS");
        gateway.collect_response(opened).await.expect("non-reflecting response");
        let requests = target.requests.lock().expect("target HTTP");
        assert!(contains(&requests[0], CANARY));
        assert!(contains(&requests[0], b"l01-private-body-canary"));
        for handshake in proxy.handshakes.lock().expect("proxy handshake").iter() {
            assert!(!contains(handshake, CANARY));
            assert!(!contains(handshake, b"l01-private-body-canary"));
        }
        let tls_bytes = proxy.tls_bytes.lock().expect("encrypted data");
        assert!(!tls_bytes.is_empty());
        assert!(!contains(&tls_bytes, CANARY));
        assert!(!contains(&tls_bytes, b"l01-private-body-canary"));
    }
}

#[tokio::test]
async fn credential_origin_binding_and_cross_origin_redirect_stripping_remain_active() {
    let final_target = target(true, vec![Reply::ok()]).await;
    let first =
        target(true, vec![Reply::redirect(final_target.url("https", "redirect-target.test"))])
            .await;
    let proxy = proxy(
        ToolNetworkProxyProtocolV1::Http,
        Behavior::Relay,
        &[first.address, final_target.address],
    )
    .await;
    let gateway = gateway(&[
        ("credential-target.test", &["8.8.8.8"]),
        ("redirect-target.test", &["8.8.4.4"]),
    ]);
    let mut resolver = credential("different-origin.test", first.address.port());
    let denied = request(first.url("https", "credential-target.test"));
    assert!(matches!(
        gateway
            .open_response(
                credential_grant(&denied, proxy.settings.clone(), &resolver),
                denied,
                None,
                Some(&resolver)
            )
            .await,
        Err(NetworkControlError::CredentialBindingMismatch)
    ));
    assert!(proxy.handshakes.lock().expect("no credential egress").is_empty());
    resolver.origin.host = "credential-target.test".into();
    let request = request(first.url("https", "credential-target.test"));
    let opened = gateway
        .open_response(
            credential_grant(&request, proxy.settings.clone(), &resolver),
            request,
            None,
            Some(&resolver),
        )
        .await
        .expect("redirect");
    gateway.collect_response(opened).await.expect("response");
    assert!(contains(&first.requests.lock().expect("authorized origin")[0], CANARY));
    assert!(!contains(&final_target.requests.lock().expect("other origin")[0], CANARY));
}
