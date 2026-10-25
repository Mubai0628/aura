use super::*;
use crate::{CoreHttpClientGateway, InMemoryNetworkGrantStore, NetworkControlOptions};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn lower_modes_cannot_issue_general_network_authority_even_with_a_matching_grant() {
    for mode in [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite] {
        let (original, _) = fixture();
        let mut snapshot = original.permission_snapshot.clone();
        snapshot.mode = mode;
        let id = aura_domain_common::PermissionGrantId::new();
        snapshot.active_grant_ids.push(id);
        let bases = [
            PermissionAllowAuthorizationBasisV1::PermissionMode { mode },
            PermissionAllowAuthorizationBasisV1::PermissionGrant {
                permission_grant_id: id,
                lifetime: aura_security_contract::PermissionGrantLifetime::Once,
            },
        ];
        for basis in bases {
            assert_eq!(
                ToolHttpRequestAuthorityV1::new(
                    original.operation_id,
                    original.operation.clone(),
                    original.operation_identity.clone(),
                    snapshot.clone(),
                    basis,
                    original.tool_execution_attempt_id,
                    UtcTimestamp::now(),
                )
                .expect_err("mode ceiling precedes grants"),
                ToolHttpGrantContractErrorV1::AuthorityMismatch,
            );
        }
    }
}

#[tokio::test]
async fn full_access_upload_uses_default_gateway_without_origin_or_data_egress_grants() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("fixture bind");
    let port = listener.local_addr().expect("address").port();
    let body = b"user-file-content: sk-proj-this-is-a-synthetic-file-canary-1234567890".to_vec();
    let expected_body = body.clone();
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut received = Vec::new();
        loop {
            let mut chunk = [0; 4096];
            let count = socket.read(&mut chunk).await.expect("read");
            assert!(count > 0, "request must finish");
            received.extend_from_slice(&chunk[..count]);
            assert!(received.len() < 65536);
            if received.ends_with(&expected_body) {
                break;
            }
        }
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .expect("response");
        received
    });
    let (mut original, mut request) = fixture();
    request.plan.requested_url = format!("http://127.0.0.1:{port}/v1/items");
    request.plan.destination = Some(ResolvedNetworkDestination {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port,
        addresses: vec!["127.0.0.1".into()],
        classes: vec![NetworkDestinationClass::Loopback],
    });
    request.plan.data_classes = vec![NetworkDataClass::UserFileContent];
    request.plan.request_data.clear();
    request.body = body;
    let NonSecretToolRuntimeFactsV1::NetworkGateway { facts } = &mut original.operation.runtime
    else {
        panic!("HTTP")
    };
    facts.exact_url = request.plan.requested_url.clone();
    facts.body_hash = CanonicalHashV1::sha256(&request.body);
    facts.body_byte_length = request.body.len() as u64;
    original.operation.runtime_binding_hash =
        original.operation.runtime.canonical_hash().expect("runtime hash");
    original.operation.scopes = vec![ResolvedScope::NetworkOrigin {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port,
    }];
    let authority = ToolHttpRequestAuthorityV1::new(
        original.operation_id,
        original.operation,
        original.operation_identity,
        original.permission_snapshot,
        original.authorization_basis,
        original.tool_execution_attempt_id,
        UtcTimestamp::now(),
    )
    .expect("exact authority");
    let grant = authority.issue(&request).expect("grant");
    let repeated = (authority.clone(), grant.clone(), request.clone());
    let gateway = CoreHttpClientGateway::new(
        Arc::new(InMemoryNetworkGrantStore::default()),
        None,
        NetworkControlOptions::default(),
    );
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        gateway.execute_tool_http(ToolHttpGatewayRequestV1 { authority, grant, request }),
    )
    .await
    .expect("bounded fixture")
    .expect("upload without repeat approval");
    assert_eq!(result.response.body, b"ok");
    let received = peer.await.expect("peer");
    assert!(!String::from_utf8_lossy(&received).to_lowercase().contains("authorization:"));
    assert!(matches!(
        gateway
            .execute_tool_http(ToolHttpGatewayRequestV1 {
                authority: repeated.0,
                grant: repeated.1,
                request: repeated.2
            })
            .await,
        Err(NetworkControlError::CoreGrantAlreadyConsumed)
    ));
}
