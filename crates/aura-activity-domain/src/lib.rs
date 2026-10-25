//! The run-information index's own contract: what a real executor may publish
//! about a tool it is actually running, and what a reader may ask for.
//!
//! WHY THIS IS ITS OWN CRATE AND NOT `aura-core::activity`
//! `aura-core::activity::AgentActivities` is the existing Web permission/cancel
//! registry. It is keyed by `SecurityOperationId`, it carries exactly one
//! capability, and it decides whether a controlled Web operation may start and
//! how it is stopped. That is an *authority* over execution. This module decides
//! nothing: it collects start/resource/end facts that the real owners already
//! know, and answers read-only questions about them. Expanding the registry into
//! an all-tool authoritative state machine would have made one component both the
//! observer and the authority, which is exactly the coupling the design forbids.
//!
//! WHY IT DOES NOT USE `ExecutionId`
//! `aura_workspace_domain::eligibility::ExecutionId` identifies a *claim* inside
//! the common file-admission service. An activity is not a claim: a Shell with an
//! unknown write set has no claim at all and still has an activity, and a single
//! claim can cover several physical resources that are reported separately. The
//! two identities are therefore different types, in different crates, with no
//! conversion between them. Nothing here can admit, deny, stop, isolate or
//! release a resource.
//!
//! WHAT A RECORD MAY CONTAIN
//! Five groups, and no more:
//!
//! | group | purpose |
//! | --- | --- |
//! | association identity | which real call/operation this is, so a query is precise |
//! | tool and group | what is running and which independent group it belongs to |
//! | owner scope | which session/workspace it belongs to, for visibility filtering |
//! | resource relation | read / write / unknown-write-set against a real target |
//! | key-parameter allowlist | the few tool-specific facts needed to recognise the target |
//!
//! `has_no_extra_facts` is the executable form of that table: a record that grows a
//! sixth group has to change this crate, and the counterexample test names the
//! field it must not grow. Nothing in a record may carry file content, a patch, a
//! full script, output, environment or credentials.
/// The tool identity, reused from the common registry. It is the same type the
/// tool contract, the projection and the registry use, so a row names exactly the
/// tool the executor ran rather than a parallel spelling of it.
pub use aura_domain_common::ToolId;
use aura_domain_common::{
    AgentRunId, CommandInvocationId, ContentHash, ModelStepId, Revision, RuntimeInstanceId,
    SecurityOperationId, SessionId, ToolCallId, TurnTaskId, UtcTimestamp, WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;
mod signed_schema;

/// Upper bound on a single key-parameter value. The allowlist projects *keys of
/// the invocation*, never payloads; a value that does not fit is reported as
/// truncated rather than carried whole.
pub const MAX_ACTIVITY_PARAMETER_BYTES: usize = 512;
/// Upper bound on resource relations per record. One call may bind several
/// targets, but an unbounded list is a payload by another name.
pub const MAX_ACTIVITY_RESOURCES: usize = 32;
/// Upper bound on the display name of one resource.
pub const MAX_ACTIVITY_RESOURCE_DISPLAY_BYTES: usize = 512;
/// The opening facts retained per activity. Enough to explain an overlap without
/// turning the index into a log.
pub const MAX_ACTIVITY_DISTINCT_RESOURCES: usize = 8;
/// Rows in one page of a precise query.
pub const DEFAULT_ACTIVITY_PAGE_ROWS: usize = 20;
/// Hard cap on rows in one page, whatever a caller asks for.
pub const MAX_ACTIVITY_PAGE_ROWS: usize = 100;
/// Summary rows selected into one model-step snapshot.
pub const DEFAULT_ACTIVITY_SUMMARY_ROWS: usize = 12;
/// Hard cap on summary rows, so a busy runtime cannot inflate every request.
pub const MAX_ACTIVITY_SUMMARY_ROWS: usize = 32;
/// Upper bound on the rendered summary body, in bytes. The activity fragment is a
/// small tail note, not a second history.
///
/// R5: the budget covers the **whole** rendered body - the fixed header line, every row's
/// separator and the trailing newline - not merely the sum of the row bodies. A budget that
/// counted only row text would under-count what is really sent.
pub const MAX_ACTIVITY_SUMMARY_BYTES: usize = 4_096;
/// R4: the most relevant-target hints one request may carry.
///
/// The hints are a lookup aid, not data: a caller that needs more than this is not describing a
/// target set it really reached, and the bound keeps a request from turning into an unbounded key
/// construction on the read path.
pub const MAX_ACTIVITY_RELEVANT_HINTS: usize = 32;
/// R5: upper bound on an `Unproven` hint.
///
/// A hint is a bounded human-readable statement, never a payload. Without this bound a publisher
/// could place an arbitrarily long string into the index through the one field that is deliberately
/// opaque, and every reader would carry it.
pub const MAX_ACTIVITY_UNPROVEN_HINT_BYTES: usize = 256;
/// R5: upper bound on a directory-entry leaf name.
pub const MAX_ACTIVITY_LEAF_BYTES: usize = 255;

/// A stable identity for one activity.
///
/// It is derived from the publishing owner's own identities, never invented:
/// see [`ActivityId::derive`]. A process-wide counter would be a second source of
/// truth about identity, and could not be reconstructed by an independent reader.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActivityId(#[serde(with = "activity_id_hex")] [u8; 16]);

impl JsonSchema for ActivityId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ActivityId")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // The wire form is the 32-character lowercase hex body, so the schema says
        // exactly that rather than leaving the boundary unvalidated.
        schemars::json_schema!({
            "type": "string",
            "pattern": "^[0-9a-f]{32}$",
            "minLength": 32,
            "maxLength": 32,
        })
    }
}

impl ActivityId {
    /// Derives the activity identity from the real executing owner's identities.
    ///
    /// The derivation is canonical JSON over the exact facts that name the
    /// activity, hashed with the repository's canonical SHA-256. Two activities
    /// differ exactly when one of these differs, so a late `end` from an old
    /// runtime generation can never resolve to a newer generation's activity.
    #[must_use]
    pub fn derive(kind: ActivityKind, reference: &ActivityExecutionRef, generation: u32) -> Self {
        let hash = aura_domain_common::jcs::canonical_sha256(&(
            "aura.activity.v1",
            kind,
            reference.canonical_tuple(),
            generation,
        ));
        let mut id = [0u8; 16];
        if let Ok(hash) = hash {
            let hex = hash.hex();
            for (index, byte) in id.iter_mut().enumerate() {
                let part = hex.get(index * 2..index * 2 + 2).unwrap_or("00");
                *byte = u8::from_str_radix(part, 16).unwrap_or(0);
            }
        }
        Self(id)
    }

    /// Rebuilds an identity from its exact 16 bytes.
    #[must_use]
    pub const fn from_bytes(value: [u8; 16]) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn to_bytes(self) -> [u8; 16] {
        self.0
    }
}

impl fmt::Display for ActivityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "act_")?;
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

mod activity_id_hex {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

    pub(super) fn serialize<S: Serializer>(value: &[u8; 16], s: S) -> Result<S::Ok, S::Error> {
        let mut text = String::with_capacity(32);
        for byte in value {
            text.push_str(&format!("{byte:02x}"));
        }
        s.serialize_str(&text)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 16], D::Error> {
        let text = String::deserialize(d)?;
        if text.len() != 32
            || !text.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(D::Error::custom("activity id must be 32 lowercase hex characters"));
        }
        let mut out = [0u8; 16];
        for (index, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
                .map_err(|_| D::Error::custom("activity id must be lowercase hex"))?;
        }
        Ok(out)
    }
}

/// Which real executor published this activity.
///
/// The set is closed on purpose. A new publisher is a real new owner of a real
/// side effect, and it has to be named here before it can publish.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    /// A tool call executed by the Agent's own tool backend.
    AgentTool,
    /// A file mutation published by Main's filesystem service.
    MainFs,
    /// A buffer commit that really wrote bytes to disk.
    BufferCommit,
    /// A P04 workspace publication reaching its real commit point.
    P04Publication,
    /// A Shell command the Agent runs in the foreground of its own call.
    ShellForeground,
    /// A Shell process the managed background owner still holds.
    ShellManagedBackground,
}

impl ActivityKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AgentTool => "agent_tool",
            Self::MainFs => "main_fs",
            Self::BufferCommit => "buffer_commit",
            Self::P04Publication => "p04_publication",
            Self::ShellForeground => "shell_foreground",
            Self::ShellManagedBackground => "shell_managed_background",
        }
    }

    /// Whether this publisher is an Agent-owned execution. Main and the managed
    /// background owner have no Agent identity, and must never manufacture one.
    #[must_use]
    pub const fn is_agent_owned(self) -> bool {
        matches!(self, Self::AgentTool | Self::ShellForeground)
    }

    /// Whether an end for this activity may be reported by the Agent-side caller.
    ///
    /// A foreground Shell returning `Completed` proves the *call* returned. It does
    /// not prove the managed descendants stopped, which is why that case publishes
    /// under [`Self::ShellManagedBackground`] instead.
    #[must_use]
    pub const fn foreground_end_is_conclusive(self) -> bool {
        !matches!(self, Self::ShellManagedBackground)
    }
}

/// The real execution reference an activity is derived from.
///
/// Every variant names identities the publishing owner really holds. The Agent
/// variants carry the real `ToolCall`/`Operation`/`Attempt`; the non-Agent
/// variants carry the identity that owner actually has, so no Agent identity is
/// fabricated for Main, a buffer commit or a managed background process.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "owner", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActivityExecutionRef {
    /// The Agent's own tool call, bound to its real operation and attempt.
    AgentToolCall {
        tool_call_id: ToolCallId,
        operation_id: SecurityOperationId,
        attempt_ordinal: u32,
    },
    /// A file command owned by Main's filesystem service.
    ///
    /// R1: the operation identity is the real one the broker minted for this dispatch, and the
    /// command invocation is carried only when the request genuinely has one. An earlier version
    /// byte-converted a `CommandInvocationId` into a `MutationJournalId` and presented it as a
    /// journal identity Main never owned; that field is gone rather than re-typed, because a
    /// fabricated identity is worse than an absent one.
    MainFileMutation {
        operation_id: SecurityOperationId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command_invocation_id: Option<CommandInvocationId>,
    },
    /// A buffer commit identified by its own editor reservation and revision.
    BufferCommit {
        reservation_id: aura_domain_common::EditorReservationId,
        buffer_revision: Revision,
    },
    /// A P04 workspace publication identified by Main's lifecycle operation.
    P04Publication {
        lifecycle_operation_id: aura_domain_common::WorkspaceLifecycleOperationId,
        publication_id: aura_domain_common::DeferredMutationId,
    },
    /// A real P04 file publication; a correlation is not a lifecycle or deferred-mutation ID.
    P04FilePublication {
        correlation_id: aura_domain_common::CorrelationId,
        operation_id: Option<SecurityOperationId>,
    },
    /// A managed Shell execution identified by its real reserved attempt.
    ShellExecution { operation_id: SecurityOperationId, attempt_ordinal: u32 },
    /// A Shell invocation owned by the Agent's command execution service.
    ShellInvocation {
        invocation_id: CommandInvocationId,
        command_id: aura_domain_common::CommandId,
    },
}

impl ActivityExecutionRef {
    /// The canonical tuple this reference hashes from. It is also what a reader
    /// echoes back, so a query can name the exact real execution it saw.
    #[must_use]
    pub fn canonical_tuple(&self) -> (String, String, u64) {
        match self {
            Self::AgentToolCall { tool_call_id, operation_id, attempt_ordinal } => (
                "agent_tool_call".to_owned(),
                format!("{tool_call_id}|{operation_id}"),
                u64::from(*attempt_ordinal),
            ),
            Self::MainFileMutation { operation_id, command_invocation_id } => (
                "main_file_mutation".to_owned(),
                match command_invocation_id {
                    Some(invocation) => format!("{operation_id}|{invocation}"),
                    None => operation_id.to_string(),
                },
                0,
            ),
            Self::BufferCommit { reservation_id, buffer_revision } => {
                ("buffer_commit".to_owned(), reservation_id.to_string(), buffer_revision.value())
            }
            Self::P04Publication { lifecycle_operation_id, publication_id } => (
                "p04_publication".to_owned(),
                format!("{lifecycle_operation_id}|{publication_id}"),
                0,
            ),
            Self::P04FilePublication { correlation_id, operation_id } => {
                ("p04_file_publication".into(), format!("{correlation_id}|{operation_id:?}"), 0)
            }
            Self::ShellExecution { operation_id, attempt_ordinal } => {
                ("shell_execution".into(), operation_id.to_string(), u64::from(*attempt_ordinal))
            }
            Self::ShellInvocation { invocation_id, command_id } => {
                ("shell_invocation".to_owned(), format!("{invocation_id}|{command_id}"), 0)
            }
        }
    }

    /// The real operation identity, when this owner has one.
    #[must_use]
    pub const fn operation_id(&self) -> Option<SecurityOperationId> {
        match self {
            Self::AgentToolCall { operation_id, .. }
            | Self::MainFileMutation { operation_id, .. }
            | Self::ShellExecution { operation_id, .. } => Some(*operation_id),
            _ => None,
        }
    }

    /// The real tool call identity, when this owner has one.
    #[must_use]
    pub const fn tool_call_id(&self) -> Option<ToolCallId> {
        match self {
            Self::AgentToolCall { tool_call_id, .. } => Some(*tool_call_id),
            _ => None,
        }
    }
}

/// The owner scope of one activity. Visibility filtering reads exactly this.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityOwnerScope {
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    /// Present only for Agent-owned executions. It is `None` for Main, buffer and
    /// managed-background publishers, and nothing fills it in on their behalf.
    pub agent_run_id: Option<AgentRunId>,
    /// Present only when the publisher really has a turn.
    pub turn_task_id: Option<TurnTaskId>,
    /// Present only when the publisher really has a model step.
    pub model_step_id: Option<ModelStepId>,
    pub runtime_instance_id: RuntimeInstanceId,
}

/// What kind of access the executor has against one real target.
///
/// `UnknownWriteSet` is not a placeholder for "we did not look": it is the honest
/// answer for a Shell whose write set cannot be proven. It never serialises
/// globally and it never disables Shell.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ActivityResourceRelation {
    Read,
    Write,
    UnknownWriteSet,
}

/// The physical resource a relation applies to.
///
/// These are A's already-verified facts, republished by the real owner. The
/// publication path copies them; it does not re-open the path, re-resolve the
/// identity or consult the journal to learn them.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActivityResourceTarget {
    /// The full identity and verified ancestry copied from the execution boundary.
    VerifiedObject {
        identity: aura_domain_common::FileIdentity,
        #[serde(default)]
        ancestors: Vec<aura_domain_common::FileIdentity>,
        subtree: bool,
    },
    /// An existing or prospective entry below an already verified parent.
    VerifiedEntry {
        parent: aura_domain_common::FileIdentity,
        pending: Vec<String>,
        case_insensitive: bool,
        /// One-to-one name keys from the held-directory boundary. Raw names stay in `pending`.
        /// Missing facts on an insensitive target are unproven, never language-folded guesses.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comparison_keys: Option<Vec<String>>,
        #[serde(default)]
        ancestors: Vec<aura_domain_common::FileIdentity>,
    },
    /// An existing object, identified by volume and file identity.
    ExistingObject {
        volume_serial: u32,
        file_index: u64,
        /// The 128-bit file id, when the volume reports one.
        file_id128: Option<[u8; 16]>,
    },
    /// A directory entry, identified by its already-verified parent object and the
    /// real leaf name with the volume's actual casing rule applied.
    DirectoryEntry {
        parent_volume_serial: u32,
        parent_file_index: u64,
        leaf: String,
        case_insensitive: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comparison_key: Option<String>,
    },
    /// A directory scope: the object plus the subtree beneath it.
    Subtree { volume_serial: u32, file_index: u64, inclusive: bool },
    /// A target whose physical identity could not be proven. It is reported as
    /// unknown rather than being folded into "unrelated".
    Unproven {
        /// A bounded, human-readable hint. Never a credential or a payload.
        hint: String,
    },
}

impl ActivityResourceTarget {
    /// Canonical comparison facts only. This information API never grants file permissions.
    #[must_use]
    pub fn name_keys(&self) -> Option<Vec<String>> {
        match self {
            Self::VerifiedEntry { pending, case_insensitive, comparison_keys, .. } => {
                if *case_insensitive {
                    comparison_keys.clone()
                } else {
                    Some(pending.clone())
                }
            }
            Self::DirectoryEntry { leaf, case_insensitive, comparison_key, .. } => {
                if *case_insensitive {
                    comparison_key.clone().map(|key| vec![key])
                } else {
                    Some(vec![leaf.clone()])
                }
            }
            _ => None,
        }
    }

    /// Whether this target is within the fixed bounds the read path accepts (R5).
    ///
    /// A target that fails here is refused at the request boundary rather than being used to build
    /// a key: an over-long hint or an empty leaf is not a legitimate identity, and accepting it
    /// would let a caller construct an unbounded key space out of a query.
    #[must_use]
    pub fn is_within_request_bounds(&self) -> bool {
        match self {
            Self::VerifiedObject { identity, ancestors, .. } => {
                identity_is_bounded(identity)
                    && ancestors.len() <= 128
                    && ancestors.iter().all(identity_is_bounded)
            }
            Self::VerifiedEntry { parent, pending, ancestors, comparison_keys, .. } => {
                identity_is_bounded(parent)
                    && !pending.is_empty()
                    && pending.len() <= 128
                    && pending.iter().all(|part| {
                        !part.is_empty()
                            && part.len() <= MAX_ACTIVITY_LEAF_BYTES
                            && !part.contains(['\0', '/', '\\'])
                    })
                    && ancestors.len() <= 128
                    && ancestors.iter().all(identity_is_bounded)
                    && comparison_keys.as_ref().is_none_or(|keys| {
                        keys.len() == pending.len()
                            && keys.iter().zip(pending).all(|(key, name)| {
                                !key.contains(['\0', '/', '\\'])
                                    && key.len() <= MAX_ACTIVITY_LEAF_BYTES
                                    && key.encode_utf16().count() == name.encode_utf16().count()
                            })
                    })
            }
            Self::Unproven { hint } => {
                hint.len() <= MAX_ACTIVITY_UNPROVEN_HINT_BYTES && !hint.contains('\u{0}')
            }
            Self::DirectoryEntry { leaf, comparison_key, .. } => {
                !leaf.is_empty()
                    && leaf.len() <= MAX_ACTIVITY_LEAF_BYTES
                    && !leaf.contains(['\0', '/', '\\'])
                    && comparison_key.as_ref().is_none_or(|key| {
                        key.len() <= MAX_ACTIVITY_LEAF_BYTES
                            && !key.contains(['\0', '/', '\\'])
                            && key.encode_utf16().count() == leaf.encode_utf16().count()
                    })
            }
            Self::ExistingObject { .. } | Self::Subtree { .. } => true,
        }
    }

    /// Whether this target is within the bounds a **record** may publish (R5).
    #[must_use]
    pub fn is_within_record_bounds(&self) -> bool {
        self.is_within_request_bounds()
            && match self {
                Self::VerifiedEntry { case_insensitive: true, .. }
                | Self::DirectoryEntry { case_insensitive: true, .. } => self.name_keys().is_some(),
                _ => true,
            }
    }
}

fn identity_is_bounded(identity: &aura_domain_common::FileIdentity) -> bool {
    !identity.volume_serial_number.is_empty()
        && identity.volume_serial_number.len() <= 32
        && !identity.file_id.is_empty()
        && identity.file_id.len() <= 64
        && !identity.volume_serial_number.contains('\0')
        && !identity.file_id.contains('\0')
}

/// One resource relation inside a published activity.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityResource {
    pub target: ActivityResourceTarget,
    pub relation: ActivityResourceRelation,
    /// The display name the owner already had. It is display only: two different
    /// strings may name the same physical target, and the identity above decides.
    pub display_name: String,
    /// True when the display name was cut to fit the bound.
    pub display_name_truncated: bool,
}

/// One allow-listed key parameter. The list is closed per tool group.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityParameter {
    pub key: String,
    pub value: String,
    pub truncated: bool,
}

/// The tool group. Groups fail independently: one broken group cannot take
/// another group's rows out of the index.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ActivityToolGroup {
    FileRead,
    FileWrite,
    Shell,
    Web,
    Other,
}

impl ActivityToolGroup {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FileRead => "file_read",
            Self::FileWrite => "file_write",
            Self::Shell => "shell",
            Self::Web => "web",
            Self::Other => "other",
        }
    }
}

/// One published activity. Exactly the five groups, and nothing else.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityRecord {
    /// Group 1: association identity.
    pub id: ActivityId,
    pub kind: ActivityKind,
    pub execution: ActivityExecutionRef,
    pub generation: u32,
    /// Group 2: tool and group.
    pub tool_id: ToolId,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool_name: String,
    /// Original source class of private paths and allowlisted parameters. Metadata stays separate.
    #[serde(default)]
    pub private_source: ActivityPrivateSource,
    pub tool_group: ActivityToolGroup,
    /// Group 3: owner scope.
    pub owner: ActivityOwnerScope,
    /// Group 4: the resource relation set.
    pub resources: Vec<ActivityResource>,
    /// Group 5: the key-parameter allowlist.
    pub parameters: Vec<ActivityParameter>,
    /// When the real owner observed the dispatch or background start.
    pub started_at: UtcTimestamp,
    /// The runtime generation this record was published under. Duplicated out of
    /// `owner` so a reader can filter generations without decoding the scope.
    pub runtime_instance_id: RuntimeInstanceId,
    /// Revision of the index at the instant this row was published. A caller that
    /// saw revision N can tell whether its own query is still reading that picture.
    pub published_at_revision: Revision,
}

impl ActivityRecord {
    /// The executable form of "these five groups and no more".
    ///
    /// A cross-cutting fact is only reachable if it is one of the five, so the
    /// negative test can assert the exact list it refuses to grow.
    ///
    /// R5: A MALFORMED FACT IS REFUSED AND COUNTED, AND THE GOOD ROWS ARE UNTOUCHED.
    /// The previous predicate checked only list lengths and two string sizes, so a record could
    /// publish a parameter key that is not on its group's allowlist, an unbounded `Unproven` hint or
    /// directory leaf, a generation of zero, an owner whose Agent identity contradicts its kind, or a
    /// `runtime_instance_id` that disagrees with its own owner scope. Each of those is a fact a
    /// reader would then trust. Every one is now rejected here, on the publish path, before the row
    /// enters the index - and the refusal is reported as [`ActivityPublishOutcome::Rejected`] so
    /// coverage states it rather than losing it silently.
    #[must_use]
    pub fn has_no_extra_facts(&self) -> bool {
        self.resources.len() <= MAX_ACTIVITY_RESOURCES
            && self.parameters.len() <= MAX_ACTIVITY_RESOURCES
            && self.tool_name.len() <= 128
            && !self.tool_name.contains('\0')
            && self.generation > 0
            && self.resources.iter().all(|resource| {
                resource.display_name.len() <= MAX_ACTIVITY_RESOURCE_DISPLAY_BYTES
                    && !resource.display_name.contains('\u{0}')
                    // R5: the target's own opaque fields are bounded too. A hint or leaf is not a
                    // payload channel, and an unbounded one would be carried by every reader.
                    && resource.target.is_within_record_bounds()
            })
            && self.parameters.iter().all(|parameter| {
                parameter.value.len() <= MAX_ACTIVITY_PARAMETER_BYTES
                    && !parameter.key.is_empty()
                    && parameter.key.len() <= 64
                    // R5: the key must be one this group's allowlist really names. A record that
                    // invents a key is claiming a parameter the design does not project, and the
                    // allowlist is the whole reason a script body or a credential cannot travel.
                    && ActivityParameterAllowlist::keys(self.tool_group).contains(&parameter.key.as_str())
            })
            // R5: the owner's identity must be internally consistent. An Agent-owned kind without an
            // Agent run (or the reverse) is a row whose owner cannot be decided, and a runtime that
            // disagrees with its own owner scope makes generation filtering unsound.
            && self.agent_identity_is_real()
            && self.owner.runtime_instance_id == self.runtime_instance_id
            && !self.owner.runtime_instance_id.to_string().is_empty()
    }

    /// Whether this record was published by a real Agent-owned executor. Main and
    /// the managed background owner must be `false` here.
    #[must_use]
    pub fn agent_identity_is_real(&self) -> bool {
        self.kind.is_agent_owned() == self.owner.agent_run_id.is_some()
    }

    /// Whether `end` may be applied at all for this activity.
    ///
    /// The identity check is what makes a late `end` from an older generation and a
    /// repeated `end` safe: the caller must name the exact record it is ending.
    #[must_use]
    pub fn matches_end(&self, id: ActivityId, generation: u32) -> bool {
        self.id == id && self.generation == generation
    }
}

/// The publication outcome a real executor gets back.
///
/// Publishing never fails the tool. A refusal is reported to the publisher so it
/// can record its own coverage fact, and the executor continues.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivityPublishOutcome {
    /// The fact was recorded.
    Recorded,
    /// The same fact was already recorded; a repeated publish is safe.
    Duplicate,
    /// Capacity is full. The fact was dropped and coverage records the drop.
    CapacityExhausted,
    /// The index is unavailable. The fact was dropped and coverage records it.
    Unavailable,
    /// The fact was refused as malformed. It was dropped and coverage records it.
    Rejected,
}

impl ActivityPublishOutcome {
    #[must_use]
    pub const fn was_recorded(self) -> bool {
        matches!(self, Self::Recorded | Self::Duplicate)
    }
}

/// Why the index is reporting less than everything.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityCoverageState {
    /// Every fact the publishers offered was recorded.
    Complete,
    /// Some facts were dropped, refused or truncated. The exact counts are beside
    /// this state and are never zero.
    Partial,
    /// The index could not answer at all. This is not "nothing is running".
    Unavailable,
}

/// How a snapshot got shorter than what exists.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityOmissionReason {
    /// Rows were cut by the summary row cap.
    SummaryRowLimit,
    /// The rendered body hit the byte cap.
    SummaryByteLimit,
    /// Facts were dropped because the index was at capacity.
    CapacityDropped,
    /// Facts were dropped because the index was unavailable.
    IndexUnavailable,
    /// Facts were refused as malformed.
    RejectedFact,
    /// Rows exist that this reader may not see under its visibility.
    VisibilityFiltered,
}

/// The coverage block every snapshot and query result carries.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityCoverage {
    pub state: ActivityCoverageState,
    /// Total rows the index held for the reader's scope before any cap.
    pub total_in_scope: u64,
    /// Rows the reader was not permitted to see. Present, not silently zero.
    pub visibility_filtered: u64,
    /// Rows dropped at publication because the index was full.
    pub capacity_dropped: u64,
    /// Facts dropped because the index was unavailable.
    pub unavailable_dropped: u64,
    /// Facts refused as malformed.
    pub rejected: u64,
    /// Rows omitted from this particular rendering by a cap.
    pub omitted_by_limit: u64,
    /// Why rows were omitted, in a deterministic order.
    pub omission_reasons: Vec<ActivityOmissionReason>,
    /// True when a body was cut to fit its byte bound.
    pub truncated: bool,
}

impl ActivityCoverage {
    /// An index that answered nothing. It is deliberately *not* the same value as
    /// "complete with zero rows", so an empty table can never be read as proof
    /// that no writer exists.
    #[must_use]
    pub fn unavailable() -> Self {
        Self {
            state: ActivityCoverageState::Unavailable,
            total_in_scope: 0,
            visibility_filtered: 0,
            capacity_dropped: 0,
            unavailable_dropped: 1,
            rejected: 0,
            omitted_by_limit: 0,
            omission_reasons: vec![ActivityOmissionReason::IndexUnavailable],
            truncated: false,
        }
    }

    /// Coverage for a reader that saw everything it was allowed to see.
    #[must_use]
    pub fn complete(total_in_scope: u64, visibility_filtered: u64) -> Self {
        Self {
            state: ActivityCoverageState::Complete,
            total_in_scope,
            visibility_filtered,
            capacity_dropped: 0,
            unavailable_dropped: 0,
            rejected: 0,
            omitted_by_limit: 0,
            omission_reasons: if visibility_filtered == 0 {
                Vec::new()
            } else {
                vec![ActivityOmissionReason::VisibilityFiltered]
            },
            truncated: false,
        }
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.state == ActivityCoverageState::Complete
    }
}

/// The exact picture every reader shares for one sampling instant.
///
/// A precise query, a page of one and the model summary are all projections of
/// *one* of these. They are never mixed, which is what makes "the page is
/// consistent" checkable rather than hopeful.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivitySnapshotHeader {
    /// The instant the index was actually sampled.
    pub sampled_at: UtcTimestamp,
    /// The runtime this snapshot was taken from.
    pub runtime_instance_id: RuntimeInstanceId,
    /// The index revision at that instant.
    pub revision: Revision,
    /// Coverage of this snapshot against the reader's scope.
    pub coverage: ActivityCoverage,
    /// True when the sampled revision is not the current one, so a caller knows
    /// the world moved on after the picture it is holding.
    pub stale: bool,
}

/// One row of a query or a summary.
/// Informational source classification, independent of resource and permission descriptors.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityPrivateSource {
    WorkspaceMetadata,
    #[default]
    PersonalData,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityRow {
    pub id: ActivityId,
    pub kind: ActivityKind,
    pub tool_id: ToolId,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool_name: String,
    /// Original source class of private paths and allowlisted parameters. Metadata stays separate.
    #[serde(default)]
    pub private_source: ActivityPrivateSource,
    pub tool_group: ActivityToolGroup,
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    /// Present only when the publisher really has one.
    pub agent_run_id: Option<AgentRunId>,
    /// The real execution reference, echoed so a reader can name the execution.
    pub execution: ActivityExecutionRef,
    pub resources: Vec<ActivityResource>,
    pub parameters: Vec<ActivityParameter>,
    pub started_at: UtcTimestamp,
}

/// What a caller is looking for.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "by", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActivityQuery {
    /// Every activity currently recorded for one session.
    Session,
    /// Every activity touching one physical target.
    Resource { target: ActivityResourceTarget },
    /// Every activity in one tool group.
    Group { group: ActivityToolGroup },
    /// One activity by its exact identity.
    Exact { id: ActivityId },
    /// Every activity whose write set is not provable.
    UnknownWriteSet,
}

/// One page of a precise query, bound to the snapshot it was read from.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityQueryResult {
    pub header: ActivitySnapshotHeader,
    /// The query this page answers, echoed so a page is self-describing.
    pub query: ActivityQuery,
    /// Zero-based offset of the first row in this page.
    pub offset: u32,
    /// How many rows this page requested.
    pub limit: u32,
    pub rows: Vec<ActivityRow>,
    /// True when `offset + rows.len()` is still short of the matching total.
    pub has_more: bool,
}

impl ActivityQueryResult {
    /// Whether two pages were read from the same fixed snapshot.
    #[must_use]
    pub fn same_snapshot(&self, other: &Self) -> bool {
        self.header.sampled_at == other.header.sampled_at
            && self.header.revision == other.header.revision
            && self.header.runtime_instance_id == other.header.runtime_instance_id
            && self.header.coverage == other.header.coverage
    }
}

/// A key-parameter allowlist entry: which keys a group may project, in order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivityParameterAllowlist;

impl ActivityParameterAllowlist {
    /// Per-group allowlist. A key not listed here is never projected, so a
    /// malicious or merely verbose path cannot become an arbitrary payload.
    #[must_use]
    pub const fn keys(group: ActivityToolGroup) -> &'static [&'static str] {
        match group {
            ActivityToolGroup::FileRead | ActivityToolGroup::FileWrite => {
                &["path", "expected_kind", "range"]
            }
            ActivityToolGroup::Shell => &["cwd", "entry", "invocation"],
            ActivityToolGroup::Web => &["method", "origin"],
            ActivityToolGroup::Other => &[],
        }
    }

    /// Projects one candidate key/value, refusing anything off the allowlist.
    ///
    /// A refusal is not an error: the key is simply absent from the projection.
    /// A value over the byte bound is cut and marked truncated, never carried
    /// whole and never silently re-encoded.
    #[must_use]
    pub fn project(group: ActivityToolGroup, key: &str, value: &str) -> Option<ActivityParameter> {
        if !Self::keys(group).contains(&key) {
            return None;
        }
        if value.len() <= MAX_ACTIVITY_PARAMETER_BYTES {
            return Some(ActivityParameter {
                key: key.to_owned(),
                value: value.to_owned(),
                truncated: false,
            });
        }
        let mut end = MAX_ACTIVITY_PARAMETER_BYTES;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        Some(ActivityParameter {
            key: key.to_owned(),
            value: value[..end].to_owned(),
            truncated: true,
        })
    }
}

/// The fixed ordering used by every reader, so page boundaries are stable.
#[must_use]
pub fn compare_rows(left: &ActivityRow, right: &ActivityRow) -> std::cmp::Ordering {
    left.started_at.cmp(&right.started_at).then_with(|| left.id.cmp(&right.id))
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ActivityContractError {
    #[error("activity.page_size_invalid")]
    PageSizeInvalid,
    #[error("activity.query_limit_invalid")]
    LimitInvalid,
}

impl ActivityQueryResult {
    pub fn validate(&self) -> Result<(), ActivityContractError> {
        if self.limit == 0 || self.limit as usize > MAX_ACTIVITY_PAGE_ROWS {
            return Err(ActivityContractError::LimitInvalid);
        }
        Ok(())
    }
}

/// The content hash of a rendered summary body, so a reader can prove that the
/// bytes it holds are the bytes that were frozen for the step.
#[must_use]
pub fn summary_content_hash(body: &str) -> ContentHash {
    ContentHash::sha256(body.as_bytes())
}

/// The model-facing input of the run-information query. It carries only what to look for: the
/// session, workspace and grant come from the step's own authenticated position, as the IPC route
/// resolves them, so a model cannot widen its visibility by naming them; `target` is a lookup
/// hint, not a grant. It sits beside `ActivityQueryRequestV1`, the same question from another
/// caller, so the two cannot drift.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[schemars(description = signed_schema::AGENT_ACTIVITY_QUERY_INPUT)]
#[serde(deny_unknown_fields)]
pub struct AgentActivityQueryInput {
    /// The question shape.
    pub by: AgentActivityQueryBy,
    /// The physical target, for `by = "resource"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ActivityResourceTarget>,
    /// One tool group, for `by = "group"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<ActivityToolGroup>,
    /// One exact activity id, for `by = "exact"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_id: Option<ActivityId>,
    /// Zero-based row offset within the queried set.
    #[serde(default)]
    pub expected_revision: Option<Revision>,
    #[serde(default)]
    pub offset: u32,
    /// Rows requested. Zero means the contract's default page.
    #[serde(default)]
    pub limit: u32,
}

/// Which question the model is asking (R4).
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentActivityQueryBy {
    /// Every activity this step's own session is running.
    Session,
    /// Every activity touching one physical target.
    Resource,
    /// Every activity in one tool group.
    Group,
    /// One activity by its exact identity.
    Exact,
    /// Every activity whose write set is not provable - the honest "a Shell may be writing
    /// somewhere I cannot name" question.
    UnknownWriteSet,
}

impl AgentActivityQueryInput {
    /// The wire query this input really names, or the reason it names none.
    ///
    /// The mapping is explicit rather than "whatever fields happen to be present", so a request
    /// carrying a target for `by = "group"` is refused instead of being answered as a different
    /// question than the one it asked.
    pub fn to_query(&self) -> Result<ActivityQuery, AgentActivityQueryInvalid> {
        let query = match self.by {
            AgentActivityQueryBy::Session => ActivityQuery::Session,
            AgentActivityQueryBy::UnknownWriteSet => ActivityQuery::UnknownWriteSet,
            AgentActivityQueryBy::Resource => ActivityQuery::Resource {
                target: self.target.clone().ok_or(AgentActivityQueryInvalid::MissingTarget)?,
            },
            AgentActivityQueryBy::Group => ActivityQuery::Group {
                group: self.group.ok_or(AgentActivityQueryInvalid::MissingGroup)?,
            },
            AgentActivityQueryBy::Exact => ActivityQuery::Exact {
                id: self.activity_id.ok_or(AgentActivityQueryInvalid::MissingActivityId)?,
            },
        };
        let unexpected = AgentActivityQueryInvalid::UnexpectedField;
        let allowed = match self.by {
            AgentActivityQueryBy::Session | AgentActivityQueryBy::UnknownWriteSet => {
                (false, false, false)
            }
            AgentActivityQueryBy::Resource => (true, false, false),
            AgentActivityQueryBy::Group => (false, true, false),
            AgentActivityQueryBy::Exact => (false, false, true),
        };
        if self.target.is_some() != allowed.0
            || self.group.is_some() != allowed.1
            || self.activity_id.is_some() != allowed.2
        {
            return Err(unexpected);
        }
        Ok(query)
    }

    /// The effective page size, defaulted and clamped to the fixed bound.
    #[must_use]
    pub fn effective_limit(&self) -> u32 {
        if self.limit == 0 {
            u32::try_from(DEFAULT_ACTIVITY_PAGE_ROWS).unwrap_or(20)
        } else {
            self.limit.min(u32::try_from(MAX_ACTIVITY_PAGE_ROWS).unwrap_or(100))
        }
    }
}

/// Why a model's activity query is not answerable as written (R4).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentActivityQueryInvalid {
    /// `by = "resource"` without the target it is about.
    MissingTarget,
    /// `by = "group"` without the group it is about.
    MissingGroup,
    /// `by = "exact"` without the id it is about.
    MissingActivityId,
    /// A field the chosen question shape does not use.
    UnexpectedField,
}

impl AgentActivityQueryInvalid {
    /// The corrigible code the model is told, so it can correct the call rather than guess.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingTarget => "activity.query_target_required",
            Self::MissingGroup => "activity.query_group_required",
            Self::MissingActivityId => "activity.query_activity_id_required",
            Self::UnexpectedField => "activity.query_unexpected_field",
        }
    }
}

impl std::fmt::Display for AgentActivityQueryInvalid {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.code())
    }
}

impl std::error::Error for AgentActivityQueryInvalid {}

/// R4: the model-facing summary of the run-information query tool.
pub const AGENT_ACTIVITY_QUERY_MODEL_SUMMARY_V1: &str =
    "Ask which real activities the runtime is running right now.";
/// R4: the model-facing description of the run-information query tool.
///
/// It states the two facts a model needs to use the tool correctly and cannot infer: that naming a
/// physical target is how it finds another writer of a file it just operated on, and that the answer
/// is bounded by what its own session may see.
pub const AGENT_ACTIVITY_QUERY_MODEL_DESCRIPTION_V1: &str = concat!(
    "Answers a question about what real executors are running, from the runtime's own ",
    "in-memory activity facts. Use by=resource with a verified target to find another ",
    "writer of a file you just operated on - including one in another session or workspace ",
    "that really touches the same physical object. Use by=unknown_write_set to find a Shell ",
    "whose write set cannot be proven. Use by=group or by=session to list your own running ",
    "tools, and by=exact to look one up by the id a previous answer gave you. It is ",
    "read-only: it takes no file protection, starts and stops nothing, cannot change any ",
    "tool's outcome, and cannot widen what your session may see. A matching target query ",
    "echoes your own known spelling; it does not disclose another session's private paths ",
    "or parameters. The result always reports ",
    "its coverage, so zero rows with complete coverage (nothing is running) is never the ",
    "same answer as an index that could not be read."
);

/// R4: the result of the model-executable run-information query.
///
/// It is the wire response's own page, reused verbatim rather than re-shaped: a model, the
/// Inspector and a Renderer that read the same query must not be able to disagree about what it
/// said. Nothing here is a permission decision - the page carries only rows the caller was already
/// allowed to see.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentActivityQueryResultV1 {
    /// The versioned contract this result is written under, so a reader can tell the shapes apart.
    pub contract_version: u32,
    /// The page, bound to the snapshot it was read from.
    pub page: ActivityQueryResult,
}

/// The versioned wire request for the run-information query route.
///
/// It is a Rust DTO with serde, and the TypeScript side is generated from it. The
/// request carries only what a caller really knows: which session it is asking as,
/// what it is looking for, and how much of one page it wants. It carries no authority:
/// the route resolves the caller's real workspace from its own projection, so a request
/// cannot widen its own visibility by naming one.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityQueryRequestV1 {
    pub session_id: SessionId,
    /// The query to answer.
    pub query: ActivityQuery,
    /// Not used by the query.
    #[serde(default)]
    pub expected_revision: Option<Revision>,
    /// Zero-based row offset within the queried set.
    #[serde(default)]
    pub offset: u32,
    /// Rows requested. Zero means "the default page".
    #[serde(default)]
    pub limit: u32,
    /// Physical targets the caller already named, so a row touching one of them is
    /// reported even when it belongs to another session or workspace.
    ///
    /// These are **lookup hints, not grants**. They cannot widen what the caller may
    /// see: a target is only honoured when the caller's own scope already reaches it,
    /// and the source markers on every returned row stay exactly as the publishing
    /// owner recorded them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relevant_targets: Vec<ActivityResourceTarget>,
}

impl ActivityQueryRequestV1 {
    /// The effective page size, defaulted and clamped to the fixed bound.
    #[must_use]
    pub fn effective_limit(&self) -> u32 {
        if self.limit == 0 {
            u32::try_from(DEFAULT_ACTIVITY_PAGE_ROWS).unwrap_or(20)
        } else {
            self.limit.min(u32::try_from(MAX_ACTIVITY_PAGE_ROWS).unwrap_or(100))
        }
    }
}

/// The current version of the run-information query contract.
pub const ACTIVITY_QUERY_CONTRACT_VERSION: u32 = 1;

/// The versioned wire response for the run-information query route.
///
/// The coverage block is part of the response, not an optional extra: a caller that
/// receives zero rows must be able to tell "nothing is running" from "this index could
/// not answer", and a caller that receives some rows must be able to see what was
/// dropped, filtered or truncated.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityQueryResponseV1 {
    /// The versioned contract this response is written under.
    pub contract_version: u32,
    pub result: ActivityQueryResult,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> ActivityExecutionRef {
        ActivityExecutionRef::AgentToolCall {
            tool_call_id: ToolCallId::new(),
            operation_id: SecurityOperationId::new(),
            attempt_ordinal: 1,
        }
    }

    #[test]
    fn activity_id_rejects_non_ascii_without_panicking() {
        let value = format!("汉{}", "a".repeat(29));
        assert_eq!(value.len(), 32);
        let parsed = std::panic::catch_unwind(|| {
            serde_json::from_value::<ActivityId>(serde_json::json!(value))
        });
        assert!(matches!(parsed, Ok(Err(_))));
    }

    #[test]
    fn activity_id_enforces_its_lowercase_wire_schema() {
        let id = ActivityId::from_bytes([0xaa; 16]);
        let encoded = serde_json::to_value(id).expect("encode");
        assert_eq!(serde_json::from_value::<ActivityId>(encoded.clone()).expect("decode"), id);
        assert!(
            serde_json::from_value::<ActivityId>(serde_json::json!(
                encoded.as_str().expect("hex").to_uppercase()
            ))
            .is_err()
        );
    }

    #[test]
    fn activity_identity_separates_owners_attempts_and_generations() {
        let reference = reference();
        let base = ActivityId::derive(ActivityKind::AgentTool, &reference, 1);
        assert_eq!(base, ActivityId::derive(ActivityKind::AgentTool, &reference, 1));
        assert_ne!(
            base,
            ActivityId::derive(ActivityKind::AgentTool, &reference, 2),
            "a later runtime generation is a different activity"
        );
        assert_ne!(
            base,
            ActivityId::derive(ActivityKind::ShellForeground, &reference, 1),
            "the same reference under another owner kind is a different activity"
        );
    }

    #[test]
    fn a_late_end_for_an_old_generation_does_not_match_a_new_one() {
        let reference = reference();
        let old = ActivityId::derive(ActivityKind::AgentTool, &reference, 1);
        let new = ActivityId::derive(ActivityKind::AgentTool, &reference, 7);
        let record = ActivityRecord {
            tool_name: String::new(),
            private_source: ActivityPrivateSource::PersonalData,
            id: new,
            kind: ActivityKind::AgentTool,
            execution: reference,
            generation: 7,
            tool_id: ToolId::new(),
            tool_group: ActivityToolGroup::FileRead,
            owner: ActivityOwnerScope {
                session_id: SessionId::new(),
                workspace_id: WorkspaceId::new(),
                agent_run_id: Some(AgentRunId::new()),
                turn_task_id: None,
                model_step_id: None,
                runtime_instance_id: RuntimeInstanceId::new(),
            },
            resources: vec![],
            parameters: vec![],
            started_at: UtcTimestamp::now(),
            runtime_instance_id: RuntimeInstanceId::new(),
            published_at_revision: Revision::INITIAL,
        };
        assert!(!record.matches_end(old, 1), "an old id must not resolve to the new row");
        assert!(record.matches_end(new, 7));
        assert!(!record.matches_end(new, 8), "a wrong generation is not the same activity");
    }

    #[test]
    fn the_parameter_allowlist_refuses_keys_it_does_not_know() {
        assert!(
            ActivityParameterAllowlist::project(ActivityToolGroup::Shell, "cwd", "C:/tmp")
                .is_some()
        );
        assert!(
            ActivityParameterAllowlist::project(
                ActivityToolGroup::Shell,
                "script_body",
                "rm -rf /"
            )
            .is_none(),
            "a full script body is not a key parameter"
        );
        assert!(
            ActivityParameterAllowlist::project(
                ActivityToolGroup::FileWrite,
                "path",
                &"a".repeat(MAX_ACTIVITY_PARAMETER_BYTES + 100)
            )
            .is_some_and(|parameter| parameter.truncated
                && parameter.value.len() == MAX_ACTIVITY_PARAMETER_BYTES)
        );
    }

    #[test]
    fn unavailable_coverage_is_not_the_same_as_an_empty_complete_table() {
        assert_ne!(ActivityCoverage::unavailable().state, ActivityCoverageState::Complete);
        assert_eq!(ActivityCoverage::complete(0, 0).state, ActivityCoverageState::Complete);
        assert_eq!(
            ActivityCoverage::unavailable().omission_reasons,
            vec![ActivityOmissionReason::IndexUnavailable]
        );
    }
}
