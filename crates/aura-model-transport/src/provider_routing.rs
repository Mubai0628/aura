//! Headers a request's profile asks for beyond the protocol: a key the Provider routes the
//! conversation's requests by, and Aura's own name in User-Agent instead of the HTTP library's.
use aura_domain_common::{ContentHash, SessionId};
use aura_model_domain::{ProviderRequestHeaderClass, TransportOperationContext};
use aura_provider_profile::{RequestRules, RoutingKey};

use crate::NonSecretProviderHeader;

const AURA_USER_AGENT: &str = concat!("Aura/", env!("CARGO_PKG_VERSION"));

pub(crate) fn routing_headers(
    rules: &RequestRules,
    session_id: Option<SessionId>,
    operation: &TransportOperationContext,
) -> Vec<NonSecretProviderHeader> {
    let mut headers = Vec::new();
    if rules.aura_user_agent {
        headers.push(NonSecretProviderHeader {
            name: "user-agent".to_owned(),
            value: AURA_USER_AGENT.to_owned(),
            class: ProviderRequestHeaderClass::ProtocolFixed,
        });
    }
    let Some(RoutingKey::ConversationHeader { name }) = &rules.routing_key else {
        return headers;
    };
    // A request outside a conversation is a conversation of its own, the same for each attempt.
    let conversation = match (session_id, operation) {
        (Some(session_id), _) => session_id.to_string(),
        (None, TransportOperationContext::AgentModelAttempt { model_step_id, .. }) => {
            model_step_id.to_string()
        }
        (
            None,
            TransportOperationContext::ModelAvailabilityTest { model_availability_test_id, .. },
        ) => model_availability_test_id.to_string(),
        (None, TransportOperationContext::ModelDiscovery { discovery_run_id }) => {
            discovery_run_id.to_string()
        }
    };
    // Derived, so the Provider sees no Aura record id and no creation time from its ULID.
    let hash = ContentHash::sha256(format!("{name}\n{conversation}").as_bytes());
    headers.push(NonSecretProviderHeader {
        name: name.clone(),
        value: format!("aura-{}", &hash.hex[..32]),
        class: ProviderRequestHeaderClass::SemanticRouting,
    });
    headers
}
