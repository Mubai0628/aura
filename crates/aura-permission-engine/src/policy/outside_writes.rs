//! A built-in Shell or terminal command whose text writes outside the workspace is asked every
//! time and never reused, in every mode but 完全访问, whose user was told when turning it on. The
//! command runs as the user, so nothing but the question keeps it to the workspace.
use aura_execution_contract::SecuredOperation;
use aura_security_contract::{OperationRiskFlag, PermissionMode};

pub(super) fn writes_outside(op: &SecuredOperation, mode: PermissionMode) -> bool {
    mode != PermissionMode::FullAccess
        && aura_security_contract::built_in_shell(&op.content.source, &op.content.action_kind)
        && op.content.risks.contains(&OperationRiskFlag::OutsideWorkspaceWrite)
}
