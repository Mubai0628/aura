use aura_workspace_domain::*;
use serde_json::json;
pub(crate) const PATH: &str = "fixtures/contracts/git-remotes.v1.json";
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<GitSyncInput>();
    generator.subschema_for::<GitPushInput>();
    generator.subschema_for::<GitRemoteResultV1>();
}
pub(crate) fn bytes() -> anyhow::Result<Vec<u8>> {
    let status = GitStatusResultV1 {
        branch: Some("main".into()),
        head: Some("b".repeat(40)),
        detached: false,
        upstream: Some("origin/main".into()),
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
    let result = GitRemoteResultV1 {
        kind: "push".into(),
        remote: "origin".into(),
        remote_url: "https://fixture.invalid/repo.git".into(),
        local_branch: Some("main".into()),
        remote_branch: Some("main".into()),
        refs: vec![GitRefUpdateV1 {
            reference: "refs/heads/main".into(),
            before: Some("a".repeat(40)),
            after: Some("b".repeat(40)),
            result: "pushed".into(),
        }],
        status,
        execution_notice: GIT_NATIVE_EXECUTION_NOTICE.into(),
    };
    let value = json!({"sync_inputs":[GitSyncInput::Fetch { remote: None }, GitSyncInput::Fetch { remote: Some("origin".into()) }, GitSyncInput::Pull { remote: None, branch: None }, GitSyncInput::Pull { remote: Some("origin".into()), branch: Some("review".into()) }],
        "push_inputs":[GitPushInput { remote: None, branch: None, remote_branch: None, set_upstream: false, force_with_lease: None }, GitPushInput { remote: Some("origin".into()), branch: Some("main".into()), remote_branch: Some("review".into()), set_upstream: true, force_with_lease: Some("a".repeat(40)) }], "result":result });
    let mut bytes = serde_json::to_vec_pretty(&value)?;
    bytes.push(b'\n');
    Ok(bytes)
}
