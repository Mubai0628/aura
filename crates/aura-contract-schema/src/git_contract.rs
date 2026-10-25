//! Git-owned workspace DTO registration, separate from the aggregate entry point.
use aura_workspace_domain::*;
use serde_json::json;
pub(crate) const PATH: &str = "fixtures/contracts/git-reads.v1.json";
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<aura_workspace_domain::GitRuntimeStatusV1>();
    generator.subschema_for::<aura_workspace_domain::GitInstallationResultV1>();
    generator.subschema_for::<aura_workspace_domain::GitWorkspaceReadRequestV1>();
    generator.subschema_for::<aura_workspace_domain::GitReadResultV1>();
}

pub(crate) fn bytes() -> anyhow::Result<Vec<u8>> {
    let queries = [
        GitReadInputV1::Status(GitStatusInput::default()),
        GitReadInputV1::Diff(serde_json::from_value(json!({}))?),
        GitReadInputV1::Log(serde_json::from_value(json!({}))?),
    ];
    let requests = queries
        .into_iter()
        .map(|query| {
            Ok(GitWorkspaceReadRequestV1 {
                workspace_id: "wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse()?,
                session_id: "ses_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse()?,
                query,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let results = [
        GitReadResultV1::Status(GitStatusResultV1 {
            branch: Some("main".into()),
            head: Some("a".repeat(40)),
            detached: false,
            upstream: Some("origin/main".into()),
            ahead: 1,
            behind: 0,
            operation: None,
            conflicts: vec![],
            entries: vec![GitChangeV1 {
                comparison_note: None,
                path: "汉字 space.txt".into(),
                original_path: None,
                index_status: ".".into(),
                worktree_status: "M".into(),
                conflict: false,
                untracked: false,
                submodule: false,
            }],
            branches: vec![],
            total_entries: 1,
            next_offset: None,
            truncated: false,
            submodule_contents_checked: false,
        }),
        GitReadResultV1::Diff(GitDiffResultV1 {
            warnings: vec![],
            files: vec![GitFileStatV1 {
                comparison_note: None,
                path: "汉字 space.txt".into(),
                original_path: None,
                added: Some(1),
                deleted: Some(1),
                binary: false,
            }],
            patch: "@@ -1 +1 @@\n-old\n+new\n".into(),
            offset_bytes: 0,
            next_offset: None,
            total_bytes: 26,
            truncated: false,
        }),
        GitReadResultV1::Log(GitLogResultV1 { commits: vec![], truncated: false }),
    ];
    let runtime = [
        GitRuntimeStateV1::Available,
        GitRuntimeStateV1::Missing,
        GitRuntimeStateV1::TooOld,
        GitRuntimeStateV1::Invalid,
    ]
    .map(|state| GitRuntimeStatusV1 {
        state,
        executable: None,
        version: None,
        message: "状态说明".into(),
        next_step: "重新检测".into(),
        installation_url: "https://git-scm.com/download/win".into(),
    });
    crate::pretty_json(&json!({"requests":requests,"results":results,"runtime":runtime}))
}
