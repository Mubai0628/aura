//! The R08/R09 Core-owned Built-in release. Existing tools.* identities remain
//! exact; new handlers are closed Rust identities, never Package runtime input.
mod browser_definitions;
mod command_definitions;
mod definitions;
mod entry_definitions;
mod init_definitions;
mod management;
mod media_definitions;
mod memory_definitions;
#[cfg(test)]
mod memory_tests;
pub mod revisions;
mod schema;
mod skill_definitions;
#[cfg(test)]
mod skill_tests;
#[cfg(test)]
mod terminal_tests;
use crate::{
    BuiltinCoreHandlerV1, BuiltinToolDefinitionSetError, RegistryCandidateV1,
    ToolSourceEligibilityEvidenceV1,
};
use aura_domain_common::{
    Revision, UtcTimestamp,
    jcs::{CanonicalHashV1, canonical_sha256},
};
use aura_tool_contract::*;
use aura_tool_domain::*;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const RELEASE_VERSION: &str = "aura-agent-builtins-v2";
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "handler",
    content = "registry_handler",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AgentBuiltinHandler {
    Registry(BuiltinCoreHandlerV1),
    FileRead,
    FileList,
    FileSearch,
    FileWrite,
    FilePatch,
    /// A file or a folder to the Recycle Bin, or for good when the user approves.
    FileDelete,
    FileFind,
    ShellQuery,
    ShellStop,
    PowerShell,
    Cmd,
    TerminalOpen,
    TerminalWrite,
    TerminalRead,
    TerminalClose,
    TerminalSensitiveInput,
    WebSearch,
    WebFetch,
    WebRead,
    HttpRequest,
    HttpDownload,
    ContextReadEvents,
    ContextReadToolResult,
    ContextReadCompaction,
    TodoGet,
    TodoReplace,
    TodoUpdate,
    PlanComplete,
    GoalGet,
    GoalCheck,
    GoalComplete,
    QueryUser,
    LineageListOpen,
    LineageReconcile,
    LineageReadSources,
    LineageReviewCompletion,
    NormalComplete,
    ModeDisposition,
    GitStatus,
    GitDiff,
    GitLog,
    GitCommit,
    GitBranch,
    GitSync,
    GitPush,
    MemorySearch,
    MemoryRead,
    MemoryRecent,
    MemoryCreate,
    MemoryUpdate,
    MemoryLink,
    MemoryForget,
    MemoryArchive,
    MemoryDelete,
    MemoryValidate,
    MemoryRepair,
    WorkspaceInitComplete,
    CommandsList,
    CommandsDescribe,
    CommandsSave,
    CommandsDelete,
    SkillsSearch,
    SkillsDescribe,
    SkillsLoad,
    SkillsUnload,
    SkillsListLoaded,
    SkillsReadResource,
    /// The unified entry. Admission rewrites its calls into the tool they name.
    ToolsCall,
    BrowserOpen,
    BrowserNavigate,
    BrowserSnapshot,
    BrowserScreenshot,
    BrowserClick,
    BrowserType,
    BrowserPress,
    BrowserScroll,
    BrowserHover,
    BrowserSelect,
    BrowserWait,
    BrowserConsole,
    BrowserClose,
    BrowserRequestUser,
    BrowserEvaluate,
    BrowserNetwork,
    /// One element's HTML, box and computed style, read through DevTools.
    BrowserInspect,
    /// The page's performance counters.
    BrowserPerformance,
    MediaProbe,
    MediaFrames,
}
impl AgentBuiltinHandler {
    pub fn name(self) -> &'static str {
        match self {
            Self::Registry(handler) => handler.builtin_key(),
            Self::FileRead => "fs.read",
            Self::FileList => "fs.list",
            Self::FileSearch => "fs.search",
            Self::FileWrite => "fs.write",
            Self::FilePatch => "fs.patch",
            Self::FileDelete => "fs.delete",
            Self::FileFind => "fs.find",
            Self::ShellQuery => "shell.query",
            Self::ShellStop => "shell.stop",
            Self::PowerShell => "shell.powershell",
            Self::Cmd => "shell.cmd",
            Self::TerminalOpen => "terminal.open",
            Self::TerminalWrite => "terminal.write",
            Self::TerminalRead => "terminal.read",
            Self::TerminalSensitiveInput => "terminal.request_sensitive_input",
            Self::TerminalClose => "terminal.close",
            Self::WebSearch => "web.search",
            Self::WebFetch => "web.fetch",
            Self::WebRead => "web.read",
            Self::HttpRequest => "http.request",
            Self::HttpDownload => "http.download",
            Self::ContextReadEvents => "context.read_events",
            Self::ContextReadToolResult => "context.read_tool_result",
            Self::ContextReadCompaction => "context.read_compaction",
            Self::TodoGet => "todo.get",
            Self::TodoReplace => "todo.replace",
            Self::TodoUpdate => "todo.update",
            Self::PlanComplete => "plan.complete",
            Self::GoalGet => "goal.get",
            Self::GoalCheck => "goal.check",
            Self::GoalComplete => "goal.complete",
            Self::QueryUser => "query.user_question",
            Self::LineageListOpen => "task_lineage.list_open",
            Self::LineageReconcile => "task_lineage.reconcile",
            Self::LineageReadSources => "task_lineage.read_sources",
            Self::LineageReviewCompletion => "task_lineage.review_completion",
            Self::NormalComplete => "normal.complete",
            Self::ModeDisposition => "mode.disposition",
            Self::GitStatus => "git.status",
            Self::GitDiff => "git.diff",
            Self::GitLog => "git.log",
            Self::GitCommit => "git.commit",
            Self::GitBranch => "git.branch",
            Self::GitSync => "git.sync",
            Self::GitPush => "git.push",
            Self::MemorySearch => "memory.search",
            Self::MemoryRead => "memory.read",
            Self::MemoryRecent => "memory.list_recent",
            Self::MemoryCreate => "memory.create",
            Self::MemoryUpdate => "memory.update",
            Self::MemoryLink => "memory.link",
            Self::MemoryForget => "memory.forget",
            Self::MemoryArchive => "memory.archive",
            Self::MemoryDelete => "memory.delete",
            Self::MemoryValidate => "memory.validate",
            Self::MemoryRepair => "memory.repair",
            Self::WorkspaceInitComplete => "workspace_init.complete",
            Self::CommandsList => "commands.list",
            Self::CommandsDescribe => "commands.describe",
            Self::CommandsSave => "commands.save",
            Self::CommandsDelete => "commands.delete",
            Self::SkillsSearch => "skills.search",
            Self::SkillsDescribe => "skills.describe",
            Self::SkillsLoad => "skills.load",
            Self::SkillsUnload => "skills.unload",
            Self::SkillsListLoaded => "skills.list_loaded",
            Self::SkillsReadResource => "skills.read_resource",
            Self::ToolsCall => "tools.call",
            Self::BrowserOpen => "browser.open",
            Self::BrowserNavigate => "browser.navigate",
            Self::BrowserSnapshot => "browser.snapshot",
            Self::BrowserScreenshot => "browser.screenshot",
            Self::BrowserClick => "browser.click",
            Self::BrowserType => "browser.type",
            Self::BrowserPress => "browser.press",
            Self::BrowserScroll => "browser.scroll",
            Self::BrowserHover => "browser.hover",
            Self::BrowserSelect => "browser.select",
            Self::BrowserWait => "browser.wait",
            Self::BrowserConsole => "browser.console",
            Self::BrowserClose => "browser.close",
            Self::BrowserRequestUser => "browser.request_user",
            Self::BrowserEvaluate => "browser.evaluate",
            Self::BrowserNetwork => "browser.network",
            Self::BrowserInspect => "browser.inspect",
            Self::BrowserPerformance => "browser.performance",
            Self::MediaProbe => "media.probe",
            Self::MediaFrames => "media.frames",
        }
    }
    pub fn is_control(self) -> bool {
        matches!(
            self,
            Self::PlanComplete
                | Self::GoalComplete
                | Self::LineageReviewCompletion
                | Self::NormalComplete
        )
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentBuiltinEntry {
    pub handler: AgentBuiltinHandler,
    pub definition: ToolDefinitionRecordV1,
    pub instance: ToolInstanceRecordV1,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentBuiltinManifest {
    pub version: String,
    pub revision: Revision,
    pub entries: Vec<AgentBuiltinEntry>,
}
pub struct AgentBuiltinRelease {
    manifest: AgentBuiltinManifest,
    fingerprint: CanonicalHashV1,
}
impl AgentBuiltinRelease {
    pub fn manifest(&self) -> &AgentBuiltinManifest {
        &self.manifest
    }
    pub fn fingerprint(&self) -> &CanonicalHashV1 {
        &self.fingerprint
    }
    pub fn entry(&self, name: &str) -> Option<&AgentBuiltinEntry> {
        self.manifest.entries.iter().find(|entry| entry.handler.name() == name)
    }
    pub fn candidates(&self) -> Vec<RegistryCandidateV1> {
        self.manifest
            .entries
            .iter()
            .map(|entry| RegistryCandidateV1 {
                definition: entry.definition.clone(),
                instance: entry.instance.clone(),
                active_for_unpinned: true,
                source_evidence: ToolSourceEligibilityEvidenceV1::VerifiedAgentBuiltinRelease {
                    release_manifest_hash: self.fingerprint.clone(),
                },
            })
            .collect()
    }
    pub(super) fn validate_candidate(
        &self,
        candidate: &RegistryCandidateV1,
        hash: &CanonicalHashV1,
    ) -> bool {
        *hash == self.fingerprint
            && self.manifest.entries.iter().any(|e| {
                e.definition == candidate.definition
                    && configured_identity_matches(&e.instance, &candidate.instance)
            })
    }
}

fn configured_identity_matches(
    published: &ToolInstanceRecordV1,
    configured: &ToolInstanceRecordV1,
) -> bool {
    published.tool_instance_id == configured.tool_instance_id
        && published.tool_id == configured.tool_id
        && published.tool_version == configured.tool_version
        && published.source_kind == configured.source_kind
        && published.source_contract_fingerprint == configured.source_contract_fingerprint
        && configured.entity_revision >= published.entity_revision
        && configured.overrides.validate().is_ok()
}
// Rust defines the release, the generator publishes it, and consumers verify
// the embedded artifact. Runtime configuration cannot replace either source.
const EMBEDDED: &[u8] = include_bytes!("../../../resources/tools/builtin-agent-release.v1.json");
static RELEASE: OnceLock<Result<AgentBuiltinRelease, BuiltinToolDefinitionSetError>> =
    OnceLock::new();
pub fn release() -> Result<&'static AgentBuiltinRelease, BuiltinToolDefinitionSetError> {
    RELEASE
        .get_or_init(|| {
            let manifest: AgentBuiltinManifest = serde_json::from_slice(EMBEDDED)
                .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
            // The disk artifact is still cross-checked against the Rust definition, exactly as
            // before. That check is the point: it catches a checked-in artifact that has drifted
            // from the code that is supposed to define it.
            if manifest != generate_manifest_from_rust()? {
                return Err(BuiltinToolDefinitionSetError::DefinitionMismatch);
            }
            let fingerprint = canonical_sha256(&manifest)
                .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
            Ok(AgentBuiltinRelease { manifest, fingerprint })
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// The release manifest, built from the **Rust definition** - the generator's entry point.
///
/// WHY THIS EXISTS SEPARATELY FROM [`generate_manifest`]
/// The generator writes `builtin-tool-definition-set.v1.json` from Rust source and then needs the
/// agent release built from those same entries. Reading the entries back off disk cannot work: the
/// file on disk is the artifact the generator is in the middle of producing, so on the very change
/// that adds a definition it is stale by construction, and the generator failed with
/// `DefinitionMismatch` **before** it could write the corrected artifact. That is a bootstrapping
/// cycle in which the generator's output depended on a file the generator produces.
///
/// This function takes the entries as an argument, so the generator supplies the set it just built
/// and no cycle exists. The verification that the checked-in artifacts match the Rust definition is
/// **unchanged** - it now lives in [`generate_manifest`] and [`release`], which is where a consumer
/// reads an artifact and genuinely needs the cross-check.
pub fn generate_manifest_from_entries(
    definition_set: &crate::BuiltinToolDefinitionSetV1,
) -> Result<AgentBuiltinManifest, BuiltinToolDefinitionSetError> {
    let at: UtcTimestamp = "2026-09-09T00:00:00Z"
        .parse()
        .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
    let mut entries = definition_set
        .entries()
        .iter()
        .map(|entry| management::entry(entry, at))
        .collect::<Result<Vec<_>, BuiltinToolDefinitionSetError>>()?;
    entries.extend(definitions::entries(at)?);
    entries.sort_by_key(|entry| entry.definition.tool_id);
    if !entries.windows(2).all(|pair| pair[0].definition.tool_id < pair[1].definition.tool_id)
        || entries.iter().map(|e| e.handler.name()).collect::<std::collections::BTreeSet<_>>().len()
            != entries.len()
    {
        return Err(BuiltinToolDefinitionSetError::DefinitionMismatch);
    }
    revisions::verify(&entries)?;
    Ok(AgentBuiltinManifest {
        version: RELEASE_VERSION.into(),
        revision: Revision::INITIAL,
        entries,
    })
}

/// The release manifest built from the **embedded** definition set, with the disk cross-check.
///
/// It is the consumer-facing form: it reads the checked-in definition set and refuses to proceed if
/// that artifact does not match the Rust definition.
pub fn generate_manifest() -> Result<AgentBuiltinManifest, BuiltinToolDefinitionSetError> {
    generate_manifest_from_entries(crate::embedded_builtin_tool_definition_set_v1()?)
}

/// The release manifest built from the **Rust definition**, for the generator and for the release
/// cross-check. It never reads the on-disk definition set, so it cannot be defeated by a stale one.
pub fn generate_manifest_from_rust() -> Result<AgentBuiltinManifest, BuiltinToolDefinitionSetError>
{
    generate_manifest_from_entries(&crate::built_definition_set_v1()?)
}

#[cfg(test)]
mod tests {
    use super::{RELEASE_VERSION, generate_manifest, release};

    /// The generator must always be able to build the release it publishes, and
    /// the embedded artifact must be exactly that release.
    #[test]
    fn the_generated_manifest_is_the_published_release() {
        let generated = generate_manifest().expect("the release generator must succeed");
        assert_eq!(generated.version, RELEASE_VERSION);
        let published = release().expect("the embedded release must be valid");
        assert_eq!(&generated, published.manifest());
    }

    /// Every built-in Tool must have a stable definition key and a unique
    /// instance identity. A duplicate instance would make two different Tools
    /// share one record, and the Store would then reject the release.
    #[test]
    fn every_release_entry_has_a_unique_identity_and_name() {
        let manifest = generate_manifest().expect("manifest");
        let mut names = std::collections::BTreeSet::new();
        let mut instances = std::collections::BTreeSet::new();
        for entry in &manifest.entries {
            assert!(
                names.insert(entry.handler.name()),
                "duplicate handler name {}",
                entry.handler.name()
            );
            assert!(
                instances.insert(entry.instance.tool_instance_id.to_string()),
                "duplicate instance identity for {}",
                entry.handler.name()
            );
        }
    }
}
