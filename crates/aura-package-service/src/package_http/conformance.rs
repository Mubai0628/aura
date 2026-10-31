//! HTTP Conformance: the real request builder and
//! the real result evaluator, with the network transport replaced by an in-process
//! fixture that answers what the declared result contract allows. The service the
//! package names is never contacted, and no Manifest field or user input is added.
use super::{PackageHttpEvaluation, build, evaluate, synthesize::minimal_instance};
use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{CorrelationId, PrincipalId};
use aura_security_contract::{InvocationActor, InvocationIdentity};
use aura_tool_contract::{CompiledToolSchemaV1, ToolResultContractV1};
use aura_tool_domain::{
    CanonicalToolPackageToolV1, PackageConformanceReasonV1 as Reason, PackageToolSettingsV1,
};
use aura_tool_runtime::PreparedHttpCredentialSlotV1;
use serde_json::{Value, json};

/// The fixture's answer and whether its body is a real instance of the declared shape.
struct FixtureResponse {
    status: u16,
    body: Vec<u8>,
    shape_checked: bool,
}

pub fn run_case(
    tool: &CanonicalToolPackageToolV1,
    arguments: &Value,
    settings: Option<&PackageToolSettingsV1>,
) -> (Reason, CanonicalSha256HashV1) {
    let identity = InvocationIdentity {
        principal_id: PrincipalId::new(),
        actor: InvocationActor::SystemRuntime,
        workspace_id: None,
        session_id: None,
        turn_task_id: None,
        agent_run_id: None,
        parent_agent_run_id: None,
        correlation_id: CorrelationId::new(),
        permission_snapshot_id: None,
    };
    let request = match build(tool, arguments, settings, identity) {
        Ok(request) => request,
        Err(error) => {
            return (
                Reason::InvalidProtocol,
                evidence(&json!({"stage":"request","error":error.to_string()})),
            );
        }
    };
    // A synthetic credential stands in for the user's; it is placed exactly where the
    // manifest declares and must appear nowhere else in what the fixture receives.
    let synthetic = format!(
        "aura-conformance-{}{}",
        CorrelationId::new().to_string().to_ascii_lowercase(),
        CorrelationId::new().to_string().to_ascii_lowercase()
    );
    let mut headers = request.gateway.headers.clone();
    if let Some(slot) = request.credential_slot() {
        let (name, value) = match slot {
            PreparedHttpCredentialSlotV1::BearerAuthorization { .. } => {
                ("authorization".to_owned(), format!("Bearer {synthetic}"))
            }
            PreparedHttpCredentialSlotV1::FixedHeader { header_name, .. } => {
                (header_name.clone(), synthetic.clone())
            }
        };
        headers.push((name, value));
    }
    let placed = request.credential_slot().map(|slot| match slot {
        PreparedHttpCredentialSlotV1::BearerAuthorization { .. } => "authorization".to_owned(),
        PreparedHttpCredentialSlotV1::FixedHeader { header_name, .. } => {
            header_name.to_ascii_lowercase()
        }
    });
    let leaked = request.gateway.plan.requested_url.contains(&synthetic)
        || String::from_utf8_lossy(&request.gateway.body).contains(&synthetic)
        || headers.iter().any(|(name, value)| {
            value.contains(&synthetic) && Some(name.to_ascii_lowercase()) != placed
        });
    let fixture = fixture_response(tool, &request.prepared.success_status_ranges);
    let received = json!({
        "method": request.gateway.plan.method,
        "url": request.gateway.plan.requested_url,
        "header_names": headers.iter().map(|(name, _)| name.to_ascii_lowercase()).collect::<Vec<_>>(),
        "body_sha256": CanonicalSha256HashV1::sha256(&request.gateway.body),
        "request_hash": request.prepared.request_hash,
        "credential_placed": placed,
    });
    let reason = if leaked || String::from_utf8_lossy(&fixture.body).contains(&synthetic) {
        Reason::CredentialReflected
    } else if !fixture.shape_checked {
        // Only the status path is exercised; the declared shape is checked by the
        // first real call instead of being assumed here.
        if request.prepared.status_is_success(fixture.status) {
            Reason::CompletedResultUnchecked
        } else {
            Reason::InvalidResult
        }
    } else {
        match evaluate(tool, &request.prepared.success_status_ranges, fixture.status, &fixture.body)
        {
            PackageHttpEvaluation::Completed { .. } => Reason::Completed,
            PackageHttpEvaluation::UnexpectedStatus { .. }
            | PackageHttpEvaluation::InvalidResult => Reason::InvalidResult,
        }
    };
    (
        reason,
        evidence(&json!({
            "backend": "package-http-fixture-v1",
            "received": received,
            "response": {"status": fixture.status, "body_sha256": CanonicalSha256HashV1::sha256(&fixture.body)},
            "result_shape_checked": fixture.shape_checked,
            "reason": reason,
        })),
    )
}

fn fixture_response(tool: &CanonicalToolPackageToolV1, ranges: &[(u16, u16)]) -> FixtureResponse {
    let allowed = |status: u16| ranges.iter().any(|(start, end)| (*start..=*end).contains(&status));
    let pick = |preferred: u16| {
        if allowed(preferred) {
            preferred
        } else {
            ranges.first().map_or(preferred, |(start, _)| *start)
        }
    };
    match &tool.model.result_contract {
        ToolResultContractV1::Unit => {
            FixtureResponse { status: pick(204), body: vec![], shape_checked: true }
        }
        ToolResultContractV1::Utf8Text => FixtureResponse {
            status: pick(200),
            body: b"Aura conformance fixture".to_vec(),
            shape_checked: true,
        },
        ToolResultContractV1::StructuredJson { result_schema } => {
            let document = result_schema.canonical_document();
            let synthesized = minimal_instance(&document.document).filter(|value| {
                CompiledToolSchemaV1::compile(document)
                    .is_ok_and(|validator| validator.validate(value).valid)
            });
            match synthesized {
                Some(value) => FixtureResponse {
                    status: pick(200),
                    body: serde_json::to_vec(&value).unwrap_or_default(),
                    shape_checked: true,
                },
                // An empty object is a well-formed JSON answer; whether it fits the
                // declared shape is reported as unchecked, never as a pass.
                None => FixtureResponse {
                    status: pick(200),
                    body: b"{}".to_vec(),
                    shape_checked: false,
                },
            }
        }
    }
}

fn evidence(value: &Value) -> CanonicalSha256HashV1 {
    canonical_sha256(value)
        .unwrap_or_else(|_| CanonicalSha256HashV1::sha256(b"package HTTP evidence unavailable"))
}
