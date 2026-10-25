use std::{fmt, str::FromStr};

use crate::IdParseError;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use ulid::Ulid;

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

define_id!(ToolId, "tol");

impl ToolId {
    /// A deterministic `ToolId` derived from a stable tool **name**.
    ///
    /// WHY THIS EXISTS
    /// Every other constructor mints a fresh ULID, which is right for a registry
    /// row and wrong for a *derived* identity: a reader that only knows the tool's
    /// name (for example a run-information row rebuilt from the executor's own
    /// facts) must be able to reconstruct the same id without a registry lookup and
    /// without persisting a second mapping.
    ///
    /// It is derived from the first 16 bytes of the canonical SHA-256 of the
    /// name-prefixed string, so it is stable across processes and machines. It is
    /// not a replacement for the registry: the registry still mints its own ids,
    /// and this is used only where a name is the whole identity available.
    #[must_use]
    pub fn from_stable_name(name: &str) -> Self {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(format!("aura.tool_id.v1:{name}").as_bytes());
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&digest[..16]);
        Self::from_bytes(bytes)
    }
}

define_id!(ToolCallId, "tcl");
define_id!(ToolInstanceId, "tin");
define_id!(ToolPackageId, "tpk");
define_id!(ToolDraftId, "tdr");
define_id!(ToolBatchId, "tbt");
define_id!(ToolExecutionAttemptId, "tea");
define_id!(ToolConformanceRunId, "tcr");
define_id!(ToolRegistrySnapshotId, "trs");

macro_rules! register_id_types {
    ($($id:ident),* $(,)?) => {
        pub const TOOL_ID_PREFIXES: &[&str] = &[$($id::PREFIX),*];
        /// Rust type-to-prefix facts for generated boundary validators.
        pub const TOOL_ID_TYPES: &[(&str, &str)] = &[$((stringify!($id), $id::PREFIX)),*];
    };
}
register_id_types!(
    ToolId,
    ToolCallId,
    ToolInstanceId,
    ToolPackageId,
    ToolDraftId,
    ToolBatchId,
    ToolExecutionAttemptId,
    ToolConformanceRunId,
    ToolRegistrySnapshotId,
);

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{
        TOOL_ID_PREFIXES, ToolBatchId, ToolCallId, ToolConformanceRunId, ToolDraftId,
        ToolExecutionAttemptId, ToolId, ToolInstanceId, ToolPackageId, ToolRegistrySnapshotId,
    };

    #[test]
    fn tool_id_uses_the_frozen_prefix_and_canonical_ulid() {
        let id = ToolId::from_str("tol_01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("canonical ToolId");
        assert_eq!(id.to_string(), "tol_01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert!(ToolId::from_str("tcl_01ARZ3NDEKTSV4RRFFQ69G5FAV").is_err());
        assert!(ToolId::from_str("tol_01arz3ndektsv4rrffq69g5fav").is_err());
    }

    #[test]
    fn p07_persistent_ids_use_unique_frozen_prefixes() {
        let body = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let cases = [
            ("tcl", ToolCallId::PREFIX),
            ("tin", ToolInstanceId::PREFIX),
            ("tpk", ToolPackageId::PREFIX),
            ("tdr", ToolDraftId::PREFIX),
            ("tbt", ToolBatchId::PREFIX),
            ("tea", ToolExecutionAttemptId::PREFIX),
            ("tcr", ToolConformanceRunId::PREFIX),
            ("trs", ToolRegistrySnapshotId::PREFIX),
        ];
        for (expected, actual) in cases {
            assert_eq!(actual, expected);
        }
        assert!(format!("tcl_{body}").parse::<ToolCallId>().is_ok());
        assert!(format!("tin_{body}").parse::<ToolInstanceId>().is_ok());
        assert!(format!("tpk_{body}").parse::<ToolPackageId>().is_ok());
        assert!(format!("tdr_{body}").parse::<ToolDraftId>().is_ok());
        assert!(format!("tbt_{body}").parse::<ToolBatchId>().is_ok());
        assert!(format!("tea_{body}").parse::<ToolExecutionAttemptId>().is_ok());
        assert!(format!("tcr_{body}").parse::<ToolConformanceRunId>().is_ok());
        assert!(format!("trs_{body}").parse::<ToolRegistrySnapshotId>().is_ok());
        assert!(
            format!("tea_{}", body.to_ascii_lowercase()).parse::<ToolExecutionAttemptId>().is_err()
        );
        assert!("tea_01ARZ3NDEKTSV4RRFFQ69G5FAI".parse::<ToolExecutionAttemptId>().is_err());
        assert_eq!(
            TOOL_ID_PREFIXES.iter().copied().collect::<std::collections::BTreeSet<_>>().len(),
            TOOL_ID_PREFIXES.len()
        );
    }
}
