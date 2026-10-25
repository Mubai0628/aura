use super::*;
use aura_domain_common::{MemoryId, WorkspacePath};

fn content(
    category: &str,
    importance: Option<u8>,
    confidence: Option<f64>,
) -> MemoryCandidateContent {
    MemoryCandidateContent {
        category: category.into(),
        body: "- fact".into(),
        basis: "seen in this task".into(),
        tags: vec![],
        confidence,
        importance,
    }
}

fn create(category: &str, importance: Option<u8>, confidence: Option<f64>) -> MemoryCandidate {
    MemoryCandidate::Create {
        scope: crate::MemoryScope::Project,
        path: WorkspacePath::parse("project/a.md").expect("path"),
        content: content(category, importance, confidence),
    }
}

fn category(candidate: &MemoryCandidate) -> &str {
    &candidate.content().expect("content").category
}

/// Four of the owner's failed runs were one candidate's category.
#[test]
fn a_category_is_written_the_way_the_store_keeps_it() {
    let mut output = MemoryMaintenanceOutput {
        schema_version: 1,
        candidates: vec![
            create("Known Issues", Some(3), None),
            create("Testing_notes", Some(3), None),
            create("测试", None, None),
            create("--x--", Some(3), Some(0.4)),
        ],
        events: vec![],
    };
    assert_eq!(normalize_maintenance_output(&mut output), 0);
    let categories = output.candidates.iter().map(category).collect::<Vec<_>>();
    assert_eq!(categories, ["known-issues", "testing-notes", "project", "x"]);
}

#[test]
fn a_candidate_naming_no_real_hash_is_left_out_and_the_others_stay() {
    let update = |hash: &str| MemoryCandidate::Update {
        scope: crate::MemoryScope::Project,
        target: MemoryId::new(),
        expected_hash: hash.into(),
        content: content("project", Some(3), None),
    };
    let mut output = MemoryMaintenanceOutput {
        schema_version: 1,
        candidates: vec![update("abc"), update(&"a".repeat(64)), create("project", None, None)],
        events: vec![],
    };
    assert_eq!(normalize_maintenance_output(&mut output), 1);
    assert_eq!(output.candidates.len(), 2);
}
