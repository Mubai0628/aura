//! Real WinCred + both production journals and readers, confined to fresh namespaces.
use super::*;
use crate::tool_credential_service::{ToolCredentialService, ToolSecretError};
use aura_core_base::credential_io;
use aura_domain_common::{CredentialRef, CredentialRevision};
use aura_model_service::p05_credential;
use aura_network_control::ProviderCredentialResolver;
use aura_persistence::ToolCredentialMutationIntentV1 as Intent;
use aura_tool_domain::{
    ToolCredentialChangeRequestV1, ToolCredentialCreateRequestV1, ToolCredentialManagementTargetV1,
    ToolCredentialPlacementV1, ToolCredentialScopeV1,
};
use aura_windows_platform::WindowsCredentialStore;

struct OwnedSecrets(Vec<CredentialRef>);
impl Drop for OwnedSecrets {
    fn drop(&mut self) {
        for reference in &self.0 {
            let _ = WindowsCredentialStore.delete(reference);
        }
    }
}
// Real platform writes and reads; only the final publication is interrupted.
// This leaves the normal Replace journal genuinely staged on disk.
struct InterruptFinalWrite(CredentialRef);
impl CredentialBackend for InterruptFinalWrite {
    fn write(
        &self,
        reference: &CredentialRef,
        secret: &SecretBytes,
        attrs: &[CredentialApplicationAttribute],
    ) -> Result<(), CredentialStoreError> {
        if reference == &self.0 {
            return Err(CredentialStoreError::Windows(std::io::Error::from_raw_os_error(995)));
        }
        WindowsCredentialStore.write_with_attributes(reference, secret, attrs)
    }
    fn read(
        &self,
        reference: &CredentialRef,
    ) -> Result<Option<BackendCredentialRecord>, CredentialStoreError> {
        match WindowsCredentialStore.read_record(reference) {
            Ok(record) => Ok(Some(BackendCredentialRecord {
                secret: SecretBytes::new(record.secret().expose().to_vec()),
                attributes: record.attributes().to_vec(),
            })),
            Err(CredentialStoreError::NotFound { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }
    fn delete(&self, reference: &CredentialRef) -> Result<bool, CredentialStoreError> {
        WindowsCredentialStore.delete(reference)
    }
}

fn binding(
    fixture: &CredentialFixture,
    revision: CredentialRevision,
) -> aura_network_control::ConfigurationRequestCredentialBindingV1 {
    aura_network_control::ConfigurationRequestCredentialBindingV1 {
        credential_id: fixture.credential_id,
        credential_revision: revision,
        credential_binding_fingerprint: fixture.binding.credential_binding_fingerprint.clone(),
    }
}
fn change(
    target: &ToolCredentialManagementTargetV1,
    record: &aura_tool_domain::ToolCredentialAuthorityRecordV1,
) -> ToolCredentialChangeRequestV1 {
    ToolCredentialChangeRequestV1 {
        target: target.clone(),
        credential_id: record.credential_id,
        expected_row_revision: record.row_revision,
    }
}
fn bytes(connection: &rusqlite::Connection, sql: &str) -> Vec<u8> {
    let mut statement =
        connection.prepare(sql).expect("complete the real isolated credential lifecycle");
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("complete the real isolated credential lifecycle")
        .collect::<Result<Vec<_>, _>>()
        .expect("complete the real isolated credential lifecycle");
    assert!(!rows.is_empty(), "a missing output class is not a clean scan");
    rows.join("\n").into_bytes()
}
fn scan(value: &[u8], secrets: &[&[u8]]) -> bool {
    !secrets.iter().any(|needle| value.windows(needle.len()).any(|part| part == *needle))
}

#[test]
fn s05_real_dual_journals_rotate_clear_recover_and_reject_cross_authority_reads() {
    let output =
        std::env::var_os("AURA_S05_SECRET_EVIDENCE").map(std::path::PathBuf::from).unwrap_or_else(
            || tempfile::tempdir().expect("complete the real isolated credential lifecycle").keep(),
        );
    std::fs::create_dir_all(&output).expect("complete the real isolated credential lifecycle");
    let _log = crate::CoreLogRoute::to(output.join("core.jsonl"));
    let model = CredentialFixture::new();
    let model_journal = p05_credential::P05CredentialJournal::new(model.persistence.clone());
    let model_reader =
        p05_credential::PersistentProviderCredentialResolver::new(model.persistence.clone());
    let mut projections = model.projections();
    let first_model = SecretBytes::new(format!("s05-model-{}", CredentialId::new()).into_bytes());
    let next_model =
        SecretBytes::new(format!("s05-model-rotated-{}", CredentialId::new()).into_bytes());
    let first_tool = SecretBytes::new(format!("s05-tool-{}", CredentialId::new()).into_bytes());
    let next_tool =
        SecretBytes::new(format!("s05-tool-rotated-{}", CredentialId::new()).into_bytes());
    let mut cleanup = OwnedSecrets(vec![model.binding.credential_ref.clone()]);
    let configured = model_journal
        .configure_secret(model.write_request(1), &first_model, &mut projections)
        .expect("complete the real isolated credential lifecycle");
    let model_revision = configured
        .metadata_commit
        .credential_record
        .last_committed_credential_revision
        .expect("complete the real isolated credential lifecycle");
    assert!(
        model_reader
            .resolve(&binding(&model, model_revision))
            .expect("complete the real isolated credential lifecycle")
            .secret_header_value
            .ends_with(first_model.expose())
    );
    let tool_fixture = crate::p07_test_support::BuiltinHttpCredentialFixtureV1::new()
        .expect("complete the real isolated credential lifecycle");
    let persistence = Arc::new(tool_fixture.persistence.clone());
    let instance = &tool_fixture.instance;
    let target = ToolCredentialManagementTargetV1 {
        tool_instance_id: instance.tool_instance_id,
        expected_instance_revision: instance.entity_revision,
        credential_slot_key: aura_tool_contract::CredentialSlotKey::new("api_key")
            .expect("complete the real isolated credential lifecycle"),
        placement: ToolCredentialPlacementV1::HttpBearerAuthorization,
        workspace_id: None,
        session_id: None,
    };
    let service = ToolCredentialService::new(persistence.clone());
    let create = ToolCredentialCreateRequestV1 {
        allowed_origins: Vec::new(),
        target: target.clone(),
        display_name: "S05 isolated tool credential".into(),
        scope: ToolCredentialScopeV1::User,
    };
    let create_command = CommandInvocationId::new();
    let created = service
        .mutate(create_command, Intent::Create { request: create.clone() }, Some(&first_tool))
        .expect("complete the real isolated credential lifecycle");
    let record =
        created.credential.clone().expect("complete the real isolated credential lifecycle");
    let (_, reference) = persistence
        .authorized_tool_secret_binding_v1(
            target.clone(),
            record.credential_id,
            record.credential_revision,
        )
        .expect("complete the real isolated credential lifecycle");
    cleanup.0.push(reference.clone());
    assert_eq!(
        service
            .read_authorized(target.clone(), record.credential_id, record.credential_revision)
            .expect("complete the real isolated credential lifecycle")
            .expose(),
        first_tool.expose()
    );
    assert!(service.read_authorized(target.clone(), model.credential_id, model_revision).is_err());
    let mut wrong_model = binding(&model, record.credential_revision);
    wrong_model.credential_id = record.credential_id;
    assert!(model_reader.resolve(&wrong_model).is_err());
    // Missing Tool platform state must not prevent a genuine Provider read.
    WindowsCredentialStore
        .delete(&reference)
        .expect("complete the real isolated credential lifecycle");
    assert!(matches!(
        service.read_authorized(target.clone(), record.credential_id, record.credential_revision),
        Err(ToolSecretError::Missing)
    ));
    assert!(model_reader.resolve(&binding(&model, model_revision)).is_ok());
    let rotated = service
        .mutate(
            CommandInvocationId::new(),
            Intent::Rotate { request: change(&target, &record) },
            Some(&next_tool),
        )
        .expect("complete the real isolated credential lifecycle");
    let current =
        rotated.credential.clone().expect("complete the real isolated credential lifecycle");
    let (_, current_ref) = persistence
        .authorized_tool_secret_binding_v1(
            target.clone(),
            current.credential_id,
            current.credential_revision,
        )
        .expect("complete the real isolated credential lifecycle");
    cleanup.0.push(current_ref.clone());
    assert_eq!(
        service
            .read_authorized(target.clone(), current.credential_id, current.credential_revision)
            .expect("complete the real isolated credential lifecycle")
            .expose(),
        next_tool.expose()
    );
    assert!(
        service
            .read_authorized(target.clone(), record.credential_id, record.credential_revision)
            .is_err()
    );
    // Real consumer paths with controlled platform errors, not constructed verdict enums.
    // Denial/cancellation injection is labelled separately from observed native OS failures.
    for (code, expected) in [(5, "denied"), (1223, "cancelled"), (1117, "not_obtained")] {
        let fault = credential_io::faults::inject(&current_ref, code);
        let error = service
            .read_authorized(target.clone(), current.credential_id, current.credential_revision)
            .expect_err("the isolated negative control must be refused");
        assert!(match expected {
            "denied" => matches!(error, ToolSecretError::AccessDenied),
            "cancelled" => matches!(error, ToolSecretError::Cancelled),
            _ => matches!(error, ToolSecretError::NotObtained),
        });
        assert!(model_reader.resolve(&binding(&model, model_revision)).is_ok());
        drop(fault);
        let fault = credential_io::faults::inject(&model.binding.credential_ref, code);
        assert!(model_reader.resolve(&binding(&model, model_revision)).is_err());
        assert!(
            service
                .read_authorized(target.clone(), current.credential_id, current.credential_revision)
                .is_ok()
        );
        drop(fault);
    }
    // A real staged Model journal, read by the normal interrupted-mutation recovery.
    let mut replacement = model.write_request(2);
    replacement.intent = ModelCredentialWriteIntent::Replace;
    let interrupted = CredentialJournalCoordinator {
        persistence: model.persistence.clone(),
        backend: InterruptFinalWrite(model.binding.credential_ref.clone()),
    };
    assert!(interrupted.configure_secret(replacement, &next_model, &mut projections).is_err());
    let staged = model
        .persistence
        .incomplete_credential_mutation_journals()
        .expect("complete the real isolated credential lifecycle")
        .into_iter()
        .next()
        .expect("complete the real isolated credential lifecycle");
    cleanup.0.push(
        staged.staging_credential_ref().expect("complete the real isolated credential lifecycle"),
    );
    assert_eq!(staged.state, CredentialMutationJournalState::Staged);
    let recovered = model_journal
        .recover(&mut projections)
        .expect("complete the real isolated credential lifecycle");
    let [CredentialRecoveryOutcome::Committed(committed)] = recovered.as_slice() else {
        panic!("actual staged mutation must commit once")
    };
    let replacement_revision = committed
        .metadata_commit
        .credential_record
        .last_committed_credential_revision
        .expect("complete the real isolated credential lifecycle");
    assert!(
        model_reader
            .resolve(&binding(&model, replacement_revision))
            .expect("complete the real isolated credential lifecycle")
            .secret_header_value
            .ends_with(next_model.expose())
    );
    assert!(model_reader.resolve(&binding(&model, model_revision)).is_err());
    // Tool recovery consumes its own durable intent, without replaying the input.
    let pending = persistence
        .prepare_tool_credential_mutation_v1(
            CommandInvocationId::new(),
            Intent::Rotate { request: change(&target, &current) },
        )
        .expect("complete the real isolated credential lifecycle");
    let pending_ref =
        pending.new_ref.clone().expect("complete the real isolated credential lifecycle");
    cleanup.0.push(pending_ref.clone());
    WindowsCredentialStore
        .write_with_attributes(
            &pending_ref,
            &first_tool,
            &crate::tool_credential_service::attributes(&pending)
                .expect("complete the real isolated credential lifecycle"),
        )
        .expect("complete the real isolated credential lifecycle");
    persistence
        .transition_tool_credential_mutation_v1(pending.journal_id, "prepared", "staged")
        .expect("complete the real isolated credential lifecycle");
    let recovered_outcomes = service
        .recover(Some(current.credential_id))
        .expect("complete the real isolated credential lifecycle");
    let [Ok(recovered_tool)] = recovered_outcomes.as_slice() else { panic!("real Tool recovery") };
    let recovered_tool =
        recovered_tool.credential.clone().expect("complete the real isolated credential lifecycle");
    assert_eq!(
        service
            .read_authorized(
                target.clone(),
                recovered_tool.credential_id,
                recovered_tool.credential_revision
            )
            .expect("complete the real isolated credential lifecycle")
            .expose(),
        first_tool.expose()
    );
    assert!(matches!(
        WindowsCredentialStore.read_record(&current_ref),
        Err(CredentialStoreError::NotFound { .. })
    ));
    // Tampered identity fails closed, while the Model chain remains usable.
    WindowsCredentialStore
        .write_with_attributes(&pending_ref, &first_tool, &[])
        .expect("complete the real isolated credential lifecycle");
    assert!(matches!(
        service.read_authorized(
            target.clone(),
            recovered_tool.credential_id,
            recovered_tool.credential_revision
        ),
        Err(ToolSecretError::IdentityMismatch)
    ));
    assert!(model_reader.resolve(&binding(&model, replacement_revision)).is_ok());
    WindowsCredentialStore
        .write_with_attributes(
            &pending_ref,
            &first_tool,
            &crate::tool_credential_service::attributes(&pending)
                .expect("complete the real isolated credential lifecycle"),
        )
        .expect("complete the real isolated credential lifecycle");
    let cleared = service
        .mutate(
            CommandInvocationId::new(),
            Intent::Clear { request: change(&target, &recovered_tool) },
            None,
        )
        .expect("complete the real isolated credential lifecycle");
    assert!(
        service
            .read_authorized(
                target.clone(),
                recovered_tool.credential_id,
                recovered_tool.credential_revision
            )
            .is_err()
    );
    assert!(matches!(
        WindowsCredentialStore.read_record(&pending_ref),
        Err(CredentialStoreError::NotFound { .. })
    ));
    model_journal
        .clear(
            PrepareModelCredentialClear {
                command_invocation_id: CommandInvocationId::new(),
                endpoint_id: model.endpoint_id,
                credential_id: model.credential_id,
                credential_slot: ModelCredentialSlot::ApiKey,
                expected_row_revision: ExpectedRevision::new(
                    committed.metadata_commit.credential_record.row_revision.value(),
                ),
                models_revision: Revision::new(7),
                correlation_id: CorrelationId::new(),
            },
            &mut projections,
        )
        .expect("complete the real isolated credential lifecycle");
    assert!(model_reader.resolve(&binding(&model, replacement_revision)).is_err());
    assert!(matches!(
        WindowsCredentialStore.read_record(&model.binding.credential_ref),
        Err(CredentialStoreError::NotFound { .. })
    ));
    let db = rusqlite::Connection::open(model.persistence.database_path())
        .expect("complete the real isolated credential lifecycle");
    let tool_db = rusqlite::Connection::open(persistence.database_path())
        .expect("complete the real isolated credential lifecycle");
    let mut events = bytes(&db, "SELECT payload_json FROM events WHERE payload_json IS NOT NULL");
    events
        .extend(bytes(&tool_db, "SELECT payload_json FROM events WHERE payload_json IS NOT NULL"));
    let mut projections = bytes(&db, "SELECT payload_json FROM projection_outbox");
    projections.extend(bytes(&tool_db, "SELECT payload_json FROM projection_outbox"));
    let mut outputs = vec![
        (
            "ipc",
            aura_ipc::encode_frame(&aura_ipc::WireFrame::Control(
                serde_json::to_vec(&aura_ipc::JsonRpcResponse::success(
                    CorrelationId::new(),
                    serde_json::to_value(&cleared)
                        .expect("complete the real isolated credential lifecycle"),
                ))
                .expect("complete the real isolated credential lifecycle"),
            ))
            .expect("complete the real isolated credential lifecycle")
            .to_vec(),
        ),
        (
            "logs",
            std::fs::read(output.join("core.jsonl"))
                .expect("complete the real isolated credential lifecycle"),
        ),
        ("events", events),
        ("projections", projections),
        (
            "context",
            serde_json::to_vec(
                &persistence
                    .prompt_bundle_snapshot_for_turn(tool_fixture.p06_initial.turn_task_id)
                    .expect("complete the real isolated credential lifecycle")
                    .expect("the committed context must be present"),
            )
            .expect("complete the real isolated credential lifecycle"),
        ),
    ];
    let artifacts=tool_fixture.p06_initial.prompt_bundle_artifacts.iter().flat_map(|a| {
        // Inspect this test's own stored products. Inspector access deliberately excludes
        // some prompt artifacts; do not weaken that authority to make a test readable.
        let read=tool_db.query_row("SELECT inline_bytes FROM artifacts WHERE artifact_id=?1",[a.artifact_id.to_string()],|row|row.get::<_,Option<Vec<u8>>>(0)).expect("complete the real isolated credential lifecycle").expect("every selected artifact must have obtained bytes; a missing class is not a clean scan");
        assert_eq!(read,a.bytes,"scan the persisted artifact, not a constructed replacement");read
    }).collect::<Vec<_>>();
    outputs.push(("artifacts", artifacts));
    let secrets =
        [first_model.expose(), next_model.expose(), first_tool.expose(), next_tool.expose()];
    let mut facts = Vec::new();
    for (class, body) in outputs {
        assert!(!body.is_empty(), "output class was not obtained: {class}");
        assert!(scan(&body, &secrets), "ordinary output leaked a credential: {class}");
        let original = output.join(format!("{class}.bin"));
        std::fs::write(&original, &body).expect("complete the real isolated credential lifecycle");
        for (authority, canary) in [("model", first_model.expose()), ("tool", first_tool.expose())]
        {
            let mut seeded = body.clone();
            seeded.extend_from_slice(canary);
            assert!(
                !scan(&seeded, &secrets),
                "per-class rejection control cannot detect {authority} leakage: {class}"
            );
            std::fs::write(output.join(format!("{class}.{authority}.seeded.bin")), &seeded)
                .expect("complete the real isolated credential lifecycle");
        }
        facts.push(serde_json::json!({"class":class,"bytes":body.len(),"clean":true,"seeded_rejected":true,"path":original,
            "capture":if class=="ipc" {"production frame codec over actual command result; network transport separately required"} else {"production journal outputs / committed P06 context artifacts"}}));
    }
    std::fs::write(
        output.join("facts.json"),
        serde_json::to_vec_pretty(&facts).expect("complete the real isolated credential lifecycle"),
    )
    .expect("complete the real isolated credential lifecycle");
}
