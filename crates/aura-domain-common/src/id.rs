use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;
use ulid::Ulid;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum IdParseError {
    #[error("expected an ID beginning with '{expected_prefix}_'")]
    InvalidPrefix { expected_prefix: &'static str },
    #[error("the ULID body must contain exactly 26 canonical Crockford base32 characters")]
    InvalidBody,
    #[error("the ULID body is not valid: {message}")]
    InvalidUlid { message: String },
}

macro_rules! define_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, Copy, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd)]
        pub struct $name(#[schemars(with = "String")] Ulid);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            #[must_use]
            pub fn new() -> Self {
                Self(Ulid::new())
            }

            #[must_use]
            pub const fn from_ulid(value: Ulid) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn ulid(self) -> Ulid {
                self.0
            }

            #[must_use]
            pub const fn from_bytes(value: [u8; 16]) -> Self {
                Self(Ulid::from_bytes(value))
            }

            #[must_use]
            pub const fn to_bytes(self) -> [u8; 16] {
                self.0.to_bytes()
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.debug_tuple(stringify!($name)).field(&self.to_string()).finish()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}_{}", Self::PREFIX, self.0)
            }
        }

        impl FromStr for $name {
            type Err = IdParseError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let body = value.strip_prefix(concat!($prefix, "_")).ok_or(
                    IdParseError::InvalidPrefix { expected_prefix: $prefix },
                )?;

                if body.len() != 26
                    || !body.bytes().all(|byte| {
                        byte.is_ascii_digit()
                            || matches!(byte, b'A'..=b'H' | b'J'..=b'N' | b'P'..=b'T' | b'V'..=b'Z')
                    })
                {
                    return Err(IdParseError::InvalidBody);
                }

                let parsed = body.parse::<Ulid>().map_err(|error| IdParseError::InvalidUlid {
                    message: error.to_string(),
                })?;

                if parsed.to_string() != body {
                    return Err(IdParseError::InvalidBody);
                }

                Ok(Self(parsed))
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                value.parse().map_err(D::Error::custom)
            }
        }
    };
}

define_id!(ProviderId, "prv");
define_id!(EndpointId, "end");
define_id!(ModelProfileId, "mdl");
define_id!(ModelExecutionSnapshotId, "mex");
define_id!(TransportAttemptId, "tat");
define_id!(ModelContentBlockId, "mcb");
define_id!(ModelFailureId, "mfl");
define_id!(ContextProjectionSnapshotId, "cps");
define_id!(PromptBundleSnapshotId, "pbs");
define_id!(ModelDiscoveryRunId, "mdr");
define_id!(ModelAvailabilityTestId, "mav");
define_id!(CredentialId, "crd");
define_id!(WorkspaceId, "wsp");
define_id!(SessionId, "ses");
define_id!(TurnTaskId, "trn");
define_id!(TaskLineageId, "lin");
define_id!(TaskLineageRelationId, "lrl");
define_id!(CompletionReviewId, "crv");
define_id!(SourceReadReceiptId, "srr");
define_id!(NormalCompletionRecordId, "ncp");
define_id!(NaturalCompletionRecordId, "ncr");
define_id!(TodoItemId, "tdo");
define_id!(UserQueryId, "uqr");
define_id!(AgentRunId, "agt");
define_id!(ModelStepId, "stp");
define_id!(ArtifactId, "art");
define_id!(MemoryId, "mem");
define_id!(SkillInstanceId, "skl");
define_id!(CommandId, "cmd");
define_id!(CommandInvocationId, "civ");
define_id!(RuntimeInstanceId, "run");
define_id!(IpcChannelId, "chn");
define_id!(RuntimeLeaseId, "lea");
define_id!(ReleaseBundleId, "rel");
define_id!(StreamId, "str");
define_id!(CorrelationId, "cor");
define_id!(CausationId, "cau");
define_id!(EventId, "evt");
define_id!(FileReferenceId, "fre");
define_id!(ExternalGrantId, "xgr");
define_id!(BrowserShareId, "bsh");
define_id!(AttachmentGrantId, "atg");
define_id!(DeferredMutationId, "dfm");
define_id!(MutationJournalId, "jnl");
define_id!(WorkspaceLifecycleOperationId, "wlo");
define_id!(EditorReservationId, "ers");
define_id!(RendererInstanceId, "rnd");
define_id!(DeleteConfirmationId, "dcn");
define_id!(PrincipalId, "pri");
define_id!(PermissionSnapshotId, "psn");
define_id!(PermissionGrantId, "pgr");
define_id!(SecurityOperationId, "sop");
define_id!(ApprovalRequestId, "apr");
define_id!(ApprovalDecisionId, "apd");
define_id!(TrustRecordId, "etr");
define_id!(SandboxExecutionId, "sbx");
define_id!(SandboxAclJournalId, "acl");
define_id!(HostPreparationOperationId, "hpo");
define_id!(ElevationLaunchTicketId, "elt");
define_id!(CancellationId, "can");
define_id!(NetworkRequestId, "nwr");
define_id!(NetworkGrantId, "ngr");
define_id!(QuarantineRecordId, "qnr");
define_id!(HookInvocationId, "hki");

/// Prefixes owned by `aura-domain-common`. Cross-crate collision validation is
/// performed by the contract generator before any registry artifact is written.
macro_rules! register_id_types {
    ($($id:ident),* $(,)?) => {
        pub const COMMON_ID_PREFIXES: &[&str] = &[$($id::PREFIX),*];
        /// Rust type-to-prefix facts for generated boundary validators.
        pub const COMMON_ID_TYPES: &[(&str, &str)] = &[$((stringify!($id), $id::PREFIX)),*];
    };
}
register_id_types!(
    ProviderId,
    EndpointId,
    ModelProfileId,
    ModelExecutionSnapshotId,
    TransportAttemptId,
    ModelContentBlockId,
    ModelFailureId,
    ContextProjectionSnapshotId,
    PromptBundleSnapshotId,
    ModelDiscoveryRunId,
    ModelAvailabilityTestId,
    CredentialId,
    WorkspaceId,
    SessionId,
    TurnTaskId,
    TaskLineageId,
    TaskLineageRelationId,
    CompletionReviewId,
    SourceReadReceiptId,
    NormalCompletionRecordId,
    NaturalCompletionRecordId,
    TodoItemId,
    UserQueryId,
    AgentRunId,
    ModelStepId,
    ArtifactId,
    MemoryId,
    SkillInstanceId,
    CommandId,
    CommandInvocationId,
    RuntimeInstanceId,
    IpcChannelId,
    RuntimeLeaseId,
    ReleaseBundleId,
    StreamId,
    CorrelationId,
    CausationId,
    EventId,
    FileReferenceId,
    ExternalGrantId,
    BrowserShareId,
    AttachmentGrantId,
    DeferredMutationId,
    MutationJournalId,
    WorkspaceLifecycleOperationId,
    EditorReservationId,
    RendererInstanceId,
    DeleteConfirmationId,
    PrincipalId,
    PermissionSnapshotId,
    PermissionGrantId,
    SecurityOperationId,
    ApprovalRequestId,
    ApprovalDecisionId,
    TrustRecordId,
    SandboxExecutionId,
    SandboxAclJournalId,
    HostPreparationOperationId,
    ElevationLaunchTicketId,
    CancellationId,
    NetworkRequestId,
    NetworkGrantId,
    QuarantineRecordId,
    HookInvocationId,
);

#[cfg(test)]
mod prefix_tests {
    use std::collections::BTreeSet;

    use super::COMMON_ID_PREFIXES;

    #[test]
    fn common_prefix_registry_is_unique_and_well_formed() {
        let unique = COMMON_ID_PREFIXES.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(unique.len(), COMMON_ID_PREFIXES.len());
        assert!(COMMON_ID_PREFIXES.iter().all(|prefix| {
            prefix.len() == 3 && prefix.bytes().all(|byte| byte.is_ascii_lowercase())
        }));
    }
}
