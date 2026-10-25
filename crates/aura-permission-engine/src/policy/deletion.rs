//! A deletion that does not go to the Recycle Bin is asked every
//! time and never reused. `fs.delete` asked to delete for good asks in every mode; a Shell command
//! that deletes files asks in every mode but 完全访问, whose user was told so when turning it on.
use aura_execution_contract::SecuredOperation;
use aura_security_contract::{OperationRiskFlag, PermissionMode, SecuritySourceRef};

pub(super) fn deletes_for_good(op: &SecuredOperation, mode: PermissionMode) -> bool {
    let builtin = matches!(op.content.source, SecuritySourceRef::BuiltInTool { .. });
    let permanent_delete_tool = builtin
        && op.content.action_kind == "fs.delete"
        && op.content.action.get("permanent").and_then(|value| value.as_bool()) == Some(true);
    permanent_delete_tool
        || mode != PermissionMode::FullAccess
            && op.content.risks.contains(&OperationRiskFlag::PermanentFileDeletion)
}
