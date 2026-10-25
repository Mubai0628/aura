use aura_workspace_domain::*;
use serde_json::json;
pub(crate) const PATH: &str = "fixtures/contracts/git-review.v1.json";
pub(crate) fn bytes() -> anyhow::Result<Vec<u8>> {
    let request = GitWorkspaceReadRequestV1 {
        workspace_id: "wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse()?,
        session_id: "ses_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse()?,
        query: GitReadInputV1::UntrackedDiff(GitUntrackedDiffInputV1 {
            path: "新增 文件.txt".into(),
            offset_bytes: 0,
            max_bytes: 16384,
        }),
    };
    let patch = "diff --git a/新增 文件.txt b/新增 文件.txt\n+new\n".to_owned();
    let result = GitReadResultV1::Diff(GitDiffResultV1 {
        warnings: vec![],
        files: vec![GitFileStatV1 {
            path: "新增 文件.txt".into(),
            original_path: None,
            added: Some(1),
            deleted: Some(0),
            binary: false,
            comparison_note: None,
        }],
        total_bytes: patch.len() as u64,
        patch,
        offset_bytes: 0,
        next_offset: None,
        truncated: false,
    });
    let mut bytes = serde_json::to_vec_pretty(&json!({"request": request, "result": result}))?;
    bytes.push(b'\n');
    Ok(bytes)
}
