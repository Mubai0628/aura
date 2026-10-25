use super::super::*;
use serde_json::json;

#[test]
fn branch_actions_reject_force_and_other_unknown_options() {
    assert!(
        serde_json::from_value::<GitBranchInput>(
            json!({"kind":"switch","name":"main","force":true})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<GitBranchInput>(
            json!({"kind":"create","name":"new","no_verify":true})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<GitCommitInput>(
            json!({"message":"x","staged_only":true,"amend":true})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<GitCommitInput>(
            json!({"message":"x","paths":["x"],"no_verify":true})
        )
        .is_err()
    );
}

#[test]
fn local_write_defaults_preserve_explicit_selection() {
    let commit: GitCommitInput =
        serde_json::from_value(json!({"message":"x","paths":["汉字 space.txt"]}))
            .expect("selected files");
    assert!(!commit.staged_only);
    let branch: GitBranchInput =
        serde_json::from_value(json!({"kind":"create","name":"new"})).expect("create branch");
    assert!(matches!(branch, GitBranchInput::Create { switch: false, start: None, .. }));
}
