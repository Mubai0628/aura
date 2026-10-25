use super::*;
use serde_json::json;
#[test]
fn read_defaults_and_explicit_targets_have_stable_serde_and_schema_boundaries() {
    let status: GitStatusInput = serde_json::from_value(json!({})).expect("status defaults");
    assert!(status.include_untracked && !status.include_branches);
    assert_eq!((status.offset, status.limit), (0, 200));
    let diff: GitDiffInput = serde_json::from_value(json!({})).expect("diff defaults");
    assert!(matches!(diff.target, GitDiffTargetV1::Unstaged {}));
    assert_eq!(diff.max_bytes, 16384);
    let log: GitLogInput = serde_json::from_value(json!({})).expect("log defaults");
    assert_eq!(log.limit, 20);
    for target in [
        json!({"kind":"unstaged"}),
        json!({"kind":"staged"}),
        json!({"kind":"commit","commit":"HEAD"}),
        json!({"kind":"range","from":"HEAD~1","to":"HEAD"}),
    ] {
        let typed: GitDiffTargetV1 = serde_json::from_value(target.clone()).expect("target");
        assert_eq!(serde_json::to_value(typed).expect("target round trip"), target);
    }
    for value in [
        json!({"kind":"commit"}),
        json!({"kind":"range","from":"HEAD"}),
        json!({"kind":"unstaged","program":"bad"}),
    ] {
        assert!(serde_json::from_value::<GitDiffTargetV1>(value).is_err());
    }
    assert!(serde_json::from_value::<GitStatusInput>(json!({"actor":"model"})).is_err());
    assert!(
        serde_json::from_value::<GitReadInputV1>(
            json!({"kind":"status","input":{},"channel":"main"})
        )
        .is_err()
    );
}
mod writes;

#[test]
fn desktop_untracked_query_does_not_expand_the_model_diff_input() {
    let query: GitReadInputV1 =
        serde_json::from_value(json!({"kind":"untracked_diff","input":{"path":"new.txt"}}))
            .expect("desktop query");
    let GitReadInputV1::UntrackedDiff(input) = query else {
        panic!("query variant");
    };
    assert_eq!((input.offset_bytes, input.max_bytes), (0, 16384));
    assert!(serde_json::from_value::<GitDiffInput>(json!({"path":"new.txt"})).is_err());
    assert!(serde_json::from_value::<GitDiffTargetV1>(json!({"kind":"untracked_diff"})).is_err());
    assert!(
        serde_json::from_value::<GitReadInputV1>(
            json!({"kind":"untracked_diff","input":{"path":"new.txt","actor":"model"}})
        )
        .is_err()
    );
}

#[test]
fn desktop_review_query_does_not_expand_the_model_diff_input() {
    let query: GitReadInputV1 = serde_json::from_value(
        json!({"kind":"review_diff","input":{"base":"main","ignore_whitespace":true}}),
    )
    .expect("desktop query");
    let GitReadInputV1::ReviewDiff(input) = query else {
        panic!("query variant");
    };
    assert_eq!(input.base.as_deref(), Some("main"));
    assert!(input.ignore_whitespace);
    assert_eq!((input.offset_bytes, input.max_bytes), (0, 16384));
    // The model's git.diff takes neither a base branch nor a whitespace choice.
    assert!(serde_json::from_value::<GitDiffInput>(json!({"base":"main"})).is_err());
    assert!(serde_json::from_value::<GitDiffInput>(json!({"ignore_whitespace":true})).is_err());
    assert!(serde_json::from_value::<GitDiffTargetV1>(json!({"kind":"review_diff"})).is_err());
}
