//! Local Git write fixtures remain separate from read-only desktop routes.
use aura_workspace_domain::*;
use serde_json::json;
pub(crate) const PATH: &str = "fixtures/contracts/git-writes.v1.json";
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<GitCommitInput>();
    generator.subschema_for::<GitBranchInput>();
    generator.subschema_for::<GitCommitResultV1>();
    generator.subschema_for::<GitBranchResultV1>();
}
pub(crate) fn bytes() -> anyhow::Result<Vec<u8>> {
    let commit_inputs = [
        GitCommitInput {
            message: "选定文件\n\n完整说明".into(),
            paths: vec!["汉字 space.txt".into()],
            staged_only: false,
        },
        GitCommitInput { message: "仅提交索引".into(), paths: vec![], staged_only: true },
    ];
    let branch_inputs = [
        GitBranchInput::Create {
            name: "feature/汉字".into(),
            start: Some("a".repeat(40)),
            switch: true,
        },
        GitBranchInput::Create { name: "feature/local".into(), start: None, switch: false },
        GitBranchInput::Switch { name: "main".into() },
    ];
    let status = GitStatusResultV1 {
        branch: Some("main".into()),
        head: Some("b".repeat(40)),
        detached: false,
        upstream: None,
        ahead: 0,
        behind: 0,
        operation: None,
        conflicts: vec![],
        entries: vec![],
        branches: vec![],
        total_entries: 0,
        next_offset: None,
        truncated: false,
        submodule_contents_checked: false,
    };
    let commit = GitCommitResultV1 {
        commit: GitCommitV1 {
            commit: "b".repeat(40),
            parents: vec!["a".repeat(40)],
            author: "Fixture <fixture@example.invalid>".into(),
            authored_at: "2026-10-15T00:00:00Z".into(),
            subject: "选定文件".into(),
            body: "选定文件\n\n完整说明\n".into(),
            files: vec![],
        },
        branch: Some("main".into()),
        status: status.clone(),
        execution_notice: GIT_NATIVE_EXECUTION_NOTICE.into(),
    };
    let branch = GitBranchResultV1 {
        previous_branch: Some("main".into()),
        previous_head: Some("b".repeat(40)),
        branch: "feature/汉字".into(),
        head: Some("a".repeat(40)),
        created: true,
        switched: true,
        status: GitStatusResultV1 {
            branch: Some("feature/汉字".into()),
            head: Some("a".repeat(40)),
            ..status
        },
        execution_notice: GIT_NATIVE_EXECUTION_NOTICE.into(),
    };
    let mut bytes = serde_json::to_vec_pretty(&json!({
        "commit_inputs":commit_inputs,"branch_inputs":branch_inputs,"commit_result":commit,"branch_result":branch,
    }))?;
    bytes.push(b'\n');
    Ok(bytes)
}
