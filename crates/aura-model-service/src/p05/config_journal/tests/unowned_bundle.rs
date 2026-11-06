//! A models bundle is reserved under its journal id before the journal exists. When the command
//! ends without that journal owning the bundle, the reservation must not outlive the command:
//! otherwise every later model-config write is refused as busy until Core restarts.
use std::sync::Arc;

use aura_domain_common::{CommandInvocationId, CorrelationId, CredentialId};
use aura_model_config::ModelConfigError;
use aura_windows_platform::SecretBytes;

use super::super::{ModelConfigCommandPlan, ModelConfigJournalError};
use super::fixture::{Fixture, credential_deletion_plan};
use crate::p05_credential::P05CredentialJournal;

fn failed_after_reserving() -> ModelConfigJournalError {
    ModelConfigJournalError::ModelConfig(ModelConfigError::InternalInvariant(
        "the plan failed after reserving its bundle",
    ))
}

#[test]
fn a_plan_that_fails_after_reserving_its_bundle_releases_it() {
    let fixture = Fixture::new();
    let failed = fixture.coordinator.execute(
        CommandInvocationId::new(),
        CorrelationId::new(),
        |journal_id| {
            fixture.provider_plan(journal_id, CorrelationId::new(), "Reserved, never journaled")?;
            Err::<ModelConfigCommandPlan, _>(failed_after_reserving())
        },
    );
    assert!(
        matches!(
            failed,
            Err(ModelConfigJournalError::ModelConfig(ModelConfigError::InternalInvariant(_)))
        ),
        "{failed:?}"
    );

    fixture.create_provider("After the failed plan").expect("the next write is admitted");
    assert_eq!(fixture.provider_count(), 1);
}

#[test]
fn a_credential_plan_that_fails_after_reserving_its_bundle_releases_it() {
    let fixture = Fixture::new();
    let credentials = P05CredentialJournal::new(Arc::clone(&fixture.persistence));
    let secret = SecretBytes::new(b"never-staged".to_vec());
    let failed = fixture.coordinator.execute_with_credential(
        &credentials,
        CommandInvocationId::new(),
        CorrelationId::new(),
        &secret,
        |journal_id, _child_journal_id| {
            fixture.provider_plan(journal_id, CorrelationId::new(), "Reserved, never journaled")?;
            Err(failed_after_reserving())
        },
    );
    assert!(failed.is_err(), "{failed:?}");

    fixture
        .create_provider("After the failed credential plan")
        .expect("the next write is admitted");
}

#[test]
fn a_deletion_plan_that_fails_after_reserving_its_bundle_releases_it() {
    let fixture = Fixture::new();
    let credentials = P05CredentialJournal::new(Arc::clone(&fixture.persistence));
    let failed = fixture.coordinator.execute_with_credential_deletes(
        &credentials,
        CommandInvocationId::new(),
        CorrelationId::new(),
        |journal_id| {
            fixture.provider_plan(journal_id, CorrelationId::new(), "Reserved, never journaled")?;
            Err(failed_after_reserving())
        },
    );
    assert!(failed.is_err(), "{failed:?}");

    fixture.create_provider("After the failed deletion plan").expect("the next write is admitted");
}

#[test]
fn a_deletion_plan_the_coordinator_refuses_releases_its_bundle() {
    let fixture = Fixture::new();
    let credentials = P05CredentialJournal::new(Arc::clone(&fixture.persistence));
    let credential_id = CredentialId::new();
    let refused = fixture.coordinator.execute_with_credential_deletes(
        &credentials,
        CommandInvocationId::new(),
        CorrelationId::new(),
        |journal_id| {
            let plan = fixture.provider_plan(journal_id, CorrelationId::new(), "Duplicate ids")?;
            Ok(credential_deletion_plan(plan, vec![credential_id, credential_id]))
        },
    );
    assert!(
        matches!(
            refused,
            Err(ModelConfigJournalError::Persistence(
                aura_persistence::PersistenceError::InvalidRequest(_)
            ))
        ),
        "{refused:?}"
    );

    fixture.create_provider("After the refused deletion").expect("the next write is admitted");
}

#[test]
fn a_command_another_request_already_journaled_releases_its_own_bundle() {
    let fixture = Fixture::new();
    let command_invocation_id = CommandInvocationId::new();
    let correlation_id = CorrelationId::new();
    let completion = fixture
        .coordinator
        .execute(command_invocation_id, correlation_id, |journal_id| {
            // The same command, journaled and completed by another request while this one was
            // still preparing its own bundle.
            fixture
                .coordinator
                .execute(command_invocation_id, correlation_id, |first| {
                    fixture.provider_plan(first, correlation_id, "Journaled by the first request")
                })
                .expect("the first request completes");
            fixture.provider_plan(journal_id, correlation_id, "Prepared by the second request")
        })
        .expect("the second request answers with the first one's journal");
    assert_eq!(completion.success_result["changed"], true);
    assert_eq!(fixture.provider_count(), 1);

    fixture.create_provider("After the duplicate request").expect("the next write is admitted");
    assert_eq!(fixture.provider_count(), 2);
}
