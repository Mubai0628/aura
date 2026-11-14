use aura_domain_common::ContentHash;
use aura_workspace_domain::{AgentFileWriteInput, FsExpectedState};
use serde_json::json;

#[test]
fn a_bare_logical_hash_and_the_tagged_hash_deserialize_to_the_same_guard() {
    let hash = ContentHash::blake3(b"original\n");
    for expected in [json!(hash), json!({"expected_kind":"file_hash","hash":hash})] {
        let input: AgentFileWriteInput = serde_json::from_value(json!({
            "path":"notes.txt", "text":"replacement\n", "expected":expected
        }))
        .expect("both spellings of the same hash guard");
        assert_eq!(input.expected, FsExpectedState::FileHash { hash: hash.clone() });
    }
}

#[test]
fn the_hash_shorthand_does_not_accept_invalid_hashes_or_extra_fields() {
    for expected in [
        json!({"algorithm":"blake3","hex":"bad"}),
        json!({"algorithm":"blake3","hex":"00".repeat(32),"unexpected":true}),
        json!("00".repeat(32)),
        json!(null),
    ] {
        assert!(
            serde_json::from_value::<AgentFileWriteInput>(json!({
                "path":"notes.txt", "text":"replacement\n", "expected":expected
            }))
            .is_err()
        );
    }
}
