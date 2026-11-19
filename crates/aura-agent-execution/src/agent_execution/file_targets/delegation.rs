//! Only built-in file readers can use a read scope requested by the main Agent.
use super::*;
use crate::agent_execution::protected_paths::{can_delegate_read, strip_verbatim_prefix};
use anyhow::{Context, ensure};
use aura_agent_team::{DelegatedReadBasis, DelegatedReadScope, DelegatedReads, MemberState};
use aura_permission_domain::PermissionModeTransition;
use aura_windows_platform::{NativePathInfo, inspect_existing_path};
use aura_workspace::domain_identity;
use aura_workspace_domain::{WorkspaceState, WorkspaceSummary};
use std::path::Path;

const READ: &[ResolvedCapability] = &[ResolvedCapability::ExternalFileRead];

impl AgentExecution {
    pub fn prepare_delegated_reads(
        &self,
        parent: SessionId,
        turn: TurnTaskId,
        paths: &[String],
    ) -> anyhow::Result<DelegatedReads> {
        ensure!((1..=8).contains(&paths.len()), "agents.read_paths_invalid: 只读路径最多八项。");
        ensure!(
            self.delegation_open(parent, turn)?,
            "agents.read_delegation_unavailable: 当前任务无法代批。确认已开启主 Agent 代批。"
        );
        let permission = self.persistence.session_permission_state(parent)?;
        ensure!(
            permission.transition == PermissionModeTransition::Settled,
            "agents.read_delegation_unavailable: 主会话权限正在切换，请使用切换后的权限重新检查。"
        );
        let workspace = self.persistence.workspace_of_session(parent)?;
        let mut scopes = Vec::new();
        for path in paths {
            let requested = Path::new(path);
            ensure!(
                requested.is_absolute() && !path.chars().any(char::is_control),
                "agents.read_paths_invalid: read_paths 要求有效的绝对路径。"
            );
            let target = inspect_existing_path(requested, true).context(
                "agents.read_path_unavailable: 找不到要委派的路径，或无法确认它的文件身份。",
            )?;
            ensure!(
                self.ordinary_read_target(&target),
                "agents.read_path_protected: 这项只读委派涉及受保护路径，需要用户决定。"
            );
            let selected = self.selected_workspace(workspace, permission.mode, &target)?;
            let basis = if let Some(selected) = selected {
                DelegatedReadBasis::Workspace {
                    workspace: selected.workspace_id,
                    epoch: selected.authorization_epoch,
                }
            } else {
                let covered = self.external_grants.grant_for_capabilities(
                    parent,
                    workspace,
                    &target.final_path,
                    Some(turn),
                    CorrelationId::new(),
                    READ,
                    target.is_directory,
                )?;
                let covered = covered.context(
                    "agents.read_path_not_authorized: 路径需要已有的工作区选择或文件读取授权。可以继续处理其他路径，或申请一次范围明确的目录只读授权。",
                )?;
                let record = self.external_grants.record(covered.grant_id)?;
                DelegatedReadBasis::ExternalGrant {
                    grant: record.id,
                    revision: record.revalidation_revision,
                }
            };
            let scope = DelegatedReadScope {
                path: strip_verbatim_prefix(&target.final_path.display().to_string()),
                identity: domain_identity(&target.identity),
                basis,
            };
            if !scopes.contains(&scope) {
                scopes.push(scope);
            }
        }
        Ok(DelegatedReads {
            parent_turn: turn,
            parent_permission_revision: permission.revision,
            scopes,
        })
    }

    /// Runs when planning and again at verified start. Missing evidence returns to normal policy.
    pub(super) fn delegated_read_covers(
        &self,
        operation: &SecuredOperation,
        child: SessionId,
        target: &NativePathInfo,
        enumerates: bool,
    ) -> bool {
        let content = &operation.content;
        if !is_delegatable_reader(content) || !self.ordinary_read_target(target) {
            return false;
        }
        self.live_delegated_read(operation, child, target, enumerates).unwrap_or(false)
    }

    fn live_delegated_read(
        &self,
        operation: &SecuredOperation,
        child: SessionId,
        target: &NativePathInfo,
        enumerates: bool,
    ) -> anyhow::Result<bool> {
        let member = {
            let journal = self.journal.lock()?;
            aura_agent_team::store::member_by_child(journal.database.connection(), child)?
        };
        let Some(member) = member.filter(|member| member.state == MemberState::Running) else {
            return Ok(false);
        };
        let Some(reads) = member.details.delegated_reads else { return Ok(false) };
        let parent = member.parent_session_id;
        if !self.delegation_open(parent, reads.parent_turn)? {
            return Ok(false);
        }
        let permission = self.persistence.session_permission_state(parent)?;
        if permission.revision != reads.parent_permission_revision
            || permission.transition != PermissionModeTransition::Settled
        {
            return Ok(false);
        }
        let workspace = self.persistence.workspace_of_session(parent)?;
        for scope in reads.scopes {
            let Ok(root) = inspect_existing_path(Path::new(&scope.path), true) else { continue };
            if domain_identity(&root.identity) != scope.identity
                || !self.ordinary_read_target(&root)
                || !super::native_scope::within(&root, target)
            {
                continue;
            }
            let live = match scope.basis {
                DelegatedReadBasis::Workspace { workspace: source, epoch } => {
                    let selected = self.persistence.workspace(source)?;
                    (source == workspace
                        || permission.mode == aura_security_contract::PermissionMode::FullAccess)
                        && selected.authorization_epoch == epoch
                        && self.is_selected_root(&selected, &root)
                }
                DelegatedReadBasis::ExternalGrant { grant, revision } => {
                    self.external_grants
                        .record(grant)
                        .is_ok_and(|record| record.revalidation_revision == revision)
                        && self
                            .external_grants
                            .grant_for_capabilities(
                                parent,
                                workspace,
                                &target.final_path,
                                Some(reads.parent_turn),
                                operation.content.identity.correlation_id,
                                READ,
                                enumerates,
                            )?
                            .is_some_and(|current| current.grant_id == grant)
                }
            };
            if live {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn delegation_open(&self, parent: SessionId, turn: TurnTaskId) -> anyhow::Result<bool> {
        let journal = self.journal.lock()?;
        let db = journal.database.connection();
        Ok(aura_agent_team::limits::settings(db)?.1.main_agent_approves
            && aura_persistence_v2::agent::session::load(db, parent)?.active_turn == Some(turn)
            && !aura_persistence_v2::agent::turns::load(db, turn)?.state.terminal())
    }

    fn ordinary_read_target(&self, info: &NativePathInfo) -> bool {
        can_delegate_read(&info.final_path, info.is_directory)
            && !self.control_resources.overlaps(info, info.is_directory)
            && !self.control_resources.in_aura_data(&info.final_path)
            && (!info.is_directory
                || !self
                    .control_resources
                    .data_folders()
                    .iter()
                    .any(|root| super::native_scope::within(info, root)))
    }

    fn selected_workspace(
        &self,
        parent: WorkspaceId,
        mode: aura_security_contract::PermissionMode,
        target: &NativePathInfo,
    ) -> anyhow::Result<Option<WorkspaceSummary>> {
        let current = self.persistence.workspace(parent)?;
        if self.is_selected_root(&current, target) {
            return Ok(Some(current));
        }
        if mode != aura_security_contract::PermissionMode::FullAccess {
            return Ok(None);
        }
        for path in target.final_path.ancestors().filter(|path| path.parent().is_some()) {
            let registered =
                self.persistence.find_workspace_by_root(&path.display().to_string(), None, None)?;
            if let Some(registered) =
                registered.filter(|entry| self.is_selected_root(entry, target))
            {
                return Ok(Some(registered));
            }
        }
        Ok(None)
    }

    fn is_selected_root(&self, selected: &WorkspaceSummary, target: &NativePathInfo) -> bool {
        selected.state == WorkspaceState::Online
            && self.persistence.worktree_parent(selected.workspace_id).is_none()
            && inspect_existing_path(Path::new(&selected.canonical_root), true).is_ok_and(|root| {
                selected.root_identity.as_ref() == Some(&domain_identity(&root.identity))
                    && super::native_scope::within(&root, target)
            })
    }
}

fn is_delegatable_reader(content: &aura_execution_contract::OperationContent) -> bool {
    matches!(content.source, SecuritySourceRef::BuiltInTool { .. })
        && content.network == OperationNetwork::None
        && content.risks.is_empty()
        && content.capabilities == READ
        && matches!(content.action_kind.as_str(), "fs.read" | "fs.list" | "fs.find" | "fs.search")
        && matches!(
            content.resources.as_slice(),
            [ResourceDescriptor::ExternalPath { identity: Some(_), .. }]
        )
}

#[cfg(test)]
#[test]
fn a_delegated_read_cannot_authorize_sensitive_resources_or_programs() {
    use aura_domain_common::{FileIdentity, PrincipalId, Revision, jcs::CanonicalSha256HashV1};
    use aura_execution_contract::OperationContent;
    use aura_security_contract::{InvocationActor, InvocationIdentity};

    let path = CanonicalPath {
        display: r"D:\fixtures\example.txt".to_owned(),
        utf16: r"D:\fixtures\example.txt".encode_utf16().collect(),
    };
    let identity = FileIdentity { volume_serial_number: "1".to_owned(), file_id: "2".to_owned() };
    let ordinary = ResourceDescriptor::ExternalPath {
        canonical_path: path.clone(),
        identity: Some(identity.clone()),
        parent_identity: None,
        directory: false,
        recursive: false,
        file_target: None,
    };
    let base = OperationContent {
        compatibility: Default::default(),
        network: OperationNetwork::None,
        identity: InvocationIdentity {
            actor: InvocationActor::ChildAgent,
            principal_id: PrincipalId::new(),
            workspace_id: None,
            session_id: None,
            turn_task_id: None,
            agent_run_id: None,
            parent_agent_run_id: None,
            correlation_id: CorrelationId::new(),
            permission_snapshot_id: None,
        },
        source: SecuritySourceRef::BuiltInTool {
            identity_hash: CanonicalSha256HashV1::sha256(b"test"),
        },
        source_revision_fingerprint: None,
        capabilities: READ.to_vec(),
        resources: vec![ordinary],
        risks: vec![],
        credentials: vec![],
        purpose: Some("Read example fixture".to_owned()),
        action_kind: "fs.read".to_owned(),
        action_revision: Revision::INITIAL,
        action: serde_json::json!({"path":"example.txt"}),
    };
    assert!(is_delegatable_reader(&base));
    for narrowed in [
        OperationContent { action_kind: "shell.powershell".into(), ..base.clone() },
        OperationContent {
            capabilities: vec![ResolvedCapability::ExternalFileWrite],
            ..base.clone()
        },
        OperationContent { network: OperationNetwork::ExactOrigins, ..base.clone() },
        OperationContent {
            resources: vec![ResourceDescriptor::SensitiveUserPath {
                canonical_path: path,
                identity: Some(identity),
                parent_identity: None,
                file_target: None,
            }],
            ..base
        },
    ] {
        assert!(!is_delegatable_reader(&narrowed));
    }
}
