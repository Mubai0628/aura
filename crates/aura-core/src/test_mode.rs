//! What a test asks Core to do instead of starting normally (`--test-mode=`).
use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoreTestMode {
    #[default]
    Normal,
    Incompatible,
    CrashAfterHandshake,
    DisconnectAfterReady,
    ProjectionRevisionGap,
    StartupSequenceGap,
    StallAfterHandshake,
    MigrationRecoveryRequired,
}

impl CoreTestMode {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "incompatible" => Self::Incompatible,
            "crash-after-handshake" => Self::CrashAfterHandshake,
            "disconnect-after-ready" => Self::DisconnectAfterReady,
            "projection-revision-gap" => Self::ProjectionRevisionGap,
            "startup-sequence-gap" => Self::StartupSequenceGap,
            "stall-after-handshake" => Self::StallAfterHandshake,
            "migration-recovery-required" => Self::MigrationRecoveryRequired,
            _ => bail!("unknown Core test mode: {value}"),
        })
    }
}
