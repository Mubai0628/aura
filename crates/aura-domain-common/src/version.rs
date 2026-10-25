use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(deny_unknown_fields)]
pub struct ContractVersion {
    pub major: u16,
    pub minor: u16,
}

impl ContractVersion {
    pub const CURRENT: Self = Self { major: 1, minor: 7 };

    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    pub const fn ensure_supported(self, incoming: Self) -> Result<(), ContractCompatibilityError> {
        if self.major != incoming.major {
            return Err(ContractCompatibilityError::MajorVersionMismatch {
                supported: self.major,
                incoming: incoming.major,
            });
        }

        if incoming.minor > self.minor {
            return Err(ContractCompatibilityError::MinorVersionTooNew {
                supported: self.minor,
                incoming: incoming.minor,
            });
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ContractCompatibilityError {
    #[error("contract major version {incoming} is incompatible with supported major {supported}")]
    MajorVersionMismatch { supported: u16, incoming: u16 },
    #[error("contract minor version {incoming} is newer than supported minor {supported}")]
    MinorVersionTooNew { supported: u16, incoming: u16 },
}
