//! `/init`: one closed write of the reviewed AGENTS.md, exposed only in an init task.
use super::definitions::entry;
use super::{AgentBuiltinEntry, AgentBuiltinHandler as H, BuiltinToolDefinitionSetError};
use aura_domain_common::UtcTimestamp;
use aura_security_contract::ResolvedCapability as C;
use aura_workspace_domain::{WorkspaceInitCompleteInput, WorkspaceInitCompleteResult};

pub(super) fn entries(
    at: UtcTimestamp,
) -> Result<Vec<AgentBuiltinEntry>, BuiltinToolDefinitionSetError> {
    Ok(vec![entry::<WorkspaceInitCompleteInput, WorkspaceInitCompleteResult>(
        56,
        H::WorkspaceInitComplete,
        "Write the reviewed AGENTS.md",
        "Only in a /init task. Submit the root AGENTS.md as five fixed chapters plus optional guides; Core renders the fixed English chapter headings, checks everything and writes all files together or none. Chapter text is Markdown without level-1 or level-2 headings; use ### or deeper. Every command needs evidence: repository_defined names a workspace file that contains the command exactly as written; execution_verified names a successful shell.powershell or shell.cmd call of this turn whose script contains it. Never write timestamps, absolute local paths, user names, secrets or guesses such as probably. Links must be workspace-relative and reach existing files or guides in this call. expected is {expected_kind: absent} or the latest logical_hash from fs.read; a changed file is never overwritten. Guides are written to docs/agent-guides/<name>.md. ReadOnly asks once, allow once or deny; WorkspaceReadWrite and FullAccess write without asking. A refusal lists every problem: fix them all and submit again, at most three times.",
        &[C::WorkspaceFileWrite, C::WorkspaceFileCreate],
        true,
        at,
    )?])
}
