//! Every model-config journal holds the one model-config lease until it finishes. A journal can
//! still be finishing after its config is active, so a write admitted to the config domain can
//! meet the lease: it waits a bounded time for the other journal, then reports the domain busy.
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use aura_domain_common::{CommandInvocationId, CorrelationId, CredentialId};
use aura_persistence::PersistenceError;
use aura_windows_platform::SecretBytes;

use super::super::{LEASE_WAIT, ModelConfigCommandCompletion, ModelConfigJournalError};
use super::fixture::{Fixture, credential_deletion_plan, credential_plan};
use crate::p05_credential::P05CredentialJournal;

const OTHER_JOURNAL_FINISHES_AFTER: Duration = Duration::from_millis(200);

/// Runs `write` while another journal holds the lease and finishes shortly after.
fn behind_a_finishing_journal(
    fixture: &Fixture,
    write: impl FnOnce() -> Result<ModelConfigCommandCompletion, ModelConfigJournalError>,
) -> Duration {
    let holder = fixture.hold_model_config_lease();
    let started = Instant::now();
    thread::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(OTHER_JOURNAL_FINISHES_AFTER);
            fixture.finish(holder);
        });
        write().expect("the write waits for the other journal");
    });
    started.elapsed()
}

fn assert_busy_after_the_wait(
    refused: Result<ModelConfigCommandCompletion, ModelConfigJournalError>,
    waited: Duration,
) {
    assert!(
        matches!(
            refused,
            Err(ModelConfigJournalError::Persistence(PersistenceError::MutationLeaseConflict))
        ),
        "{refused:?}"
    );
    assert!(waited >= LEASE_WAIT, "refused after {waited:?}, before the {LEASE_WAIT:?} wait");
}

#[test]
fn a_write_waits_for_another_journal_to_finish() {
    let fixture = Fixture::new();
    let waited = behind_a_finishing_journal(&fixture, || fixture.create_provider("Behind"));
    assert!(waited >= OTHER_JOURNAL_FINISHES_AFTER, "{waited:?}");
    assert_eq!(fixture.provider_count(), 1);
}

#[test]
fn a_deletion_without_credentials_waits_for_another_journal_to_finish() {
    let fixture = Fixture::new();
    let credentials = P05CredentialJournal::new(Arc::clone(&fixture.persistence));
    let waited = behind_a_finishing_journal(&fixture, || {
        fixture.coordinator.execute_with_credential_deletes(
            &credentials,
            CommandInvocationId::new(),
            CorrelationId::new(),
            |journal_id| {
                let plan = fixture.provider_plan(journal_id, CorrelationId::new(), "Behind")?;
                Ok(credential_deletion_plan(plan, Vec::new()))
            },
        )
    });
    assert!(waited >= OTHER_JOURNAL_FINISHES_AFTER, "{waited:?}");
    assert_eq!(fixture.provider_count(), 1);
}

#[test]
fn a_write_behind_a_journal_that_does_not_finish_is_refused_as_busy() {
    let fixture = Fixture::new();
    let holder = fixture.hold_model_config_lease();
    let started = Instant::now();
    let refused = fixture.create_provider("Behind a journal that does not finish");
    assert_busy_after_the_wait(refused, started.elapsed());
    assert_eq!(fixture.provider_count(), 0);

    fixture.finish(holder);
    fixture.create_provider("After the other journal").expect("the next write is admitted");
}

#[test]
fn a_credential_write_behind_a_journal_that_does_not_finish_is_refused_as_busy() {
    let fixture = Fixture::new();
    let credentials = P05CredentialJournal::new(Arc::clone(&fixture.persistence));
    let secret = SecretBytes::new(b"never-staged".to_vec());
    let holder = fixture.hold_model_config_lease();
    let started = Instant::now();
    let refused = fixture.coordinator.execute_with_credential(
        &credentials,
        CommandInvocationId::new(),
        CorrelationId::new(),
        &secret,
        |journal_id, _child_journal_id| {
            let plan = fixture.provider_plan(journal_id, CorrelationId::new(), "Behind")?;
            Ok(credential_plan(plan, CredentialId::new()))
        },
    );
    assert_busy_after_the_wait(refused, started.elapsed());

    fixture.finish(holder);
    fixture.create_provider("After the other journal").expect("the next write is admitted");
}

#[test]
fn a_credential_deletion_behind_a_journal_that_does_not_finish_is_refused_as_busy() {
    let fixture = Fixture::new();
    let credentials = P05CredentialJournal::new(Arc::clone(&fixture.persistence));
    let holder = fixture.hold_model_config_lease();
    let started = Instant::now();
    let refused = fixture.coordinator.execute_with_credential_deletes(
        &credentials,
        CommandInvocationId::new(),
        CorrelationId::new(),
        |journal_id| {
            let plan = fixture.provider_plan(journal_id, CorrelationId::new(), "Behind")?;
            Ok(credential_deletion_plan(plan, vec![CredentialId::new()]))
        },
    );
    assert_busy_after_the_wait(refused, started.elapsed());

    fixture.finish(holder);
    fixture.create_provider("After the other journal").expect("the next write is admitted");
}
