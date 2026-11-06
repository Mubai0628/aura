//! A credential delete is left to the cleanup only once its model-config change completed without
//! it; while that change is still going, the change itself owns its deletes.
use aura_domain_common::{CredentialId, EndpointId, MutationJournalId};

use crate::model_config_journal::tests::{
    commit_credential_slots, credential_slot, delete_payload, start,
};
use crate::{
    ModelConfigMutationJournalState as State, ModelCredentialDeletionReason,
    PrepareModelConfigCredentialDeleteJournalsResult,
};

#[test]
fn a_delete_is_pending_only_once_its_model_config_change_completed_without_it() {
    let (_directory, persistence) = start();
    let credential_id = CredentialId::new();
    commit_credential_slots(&persistence, vec![credential_slot(credential_id, EndpointId::new())]);
    let parent = MutationJournalId::new();
    let child = MutationJournalId::new();
    let prepared = persistence
        .prepare_model_config_with_credential_delete_journals(
            parent,
            delete_payload(parent, &[(credential_id, child)]),
            ModelCredentialDeletionReason::ProviderDeleted,
        )
        .expect("prepare the deletion");
    assert!(matches!(prepared, PrepareModelConfigCredentialDeleteJournalsResult::Prepared(_)));
    let pending = || persistence.pending_credential_deletes().expect("pending deletes");
    let step = |from, to| {
        persistence.transition_model_config_mutation_journal(parent, from, to, None).expect("step")
    };
    assert!(pending().is_empty(), "not published yet");

    step(State::Prepared, State::ConfigPublished);
    persistence.begin_model_config_credential_deletes(parent).expect("stage the delete");
    // Credential Manager did not delete the credential: its delete stays staged.
    step(State::ConfigPublished, State::CredentialMutationCompleted);
    persistence.commit_model_config_mutation_metadata(parent).expect("commit the deletion");
    assert!(pending().is_empty(), "the change is still being made");
    step(State::MetadataCommitted, State::CleanupPending);
    assert!(pending().is_empty(), "the change is still cleaning up its own deletes");

    step(State::CleanupPending, State::Completed);
    let left = pending();
    assert_eq!(left.iter().map(|delete| delete.journal_id).collect::<Vec<_>>(), [child]);
    assert_eq!(left[0].parent_journal_id, Some(parent));
}
