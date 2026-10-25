use serde::{Deserialize, Serialize};

use super::Limits;

/// The stage whose owner-approved budget a real call is charged to.
///
/// Each stage keeps its own account ledger. A ledger records its cumulative cap when it is
/// created and refuses a different one later, so `S10`'s larger budget cannot widen `S09`'s ledger,
/// and `S09`'s history stays exactly as it was accepted.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum BudgetStage {
    /// At most 50,000,000 tokens across the stage.
    #[default]
    S09,
    /// The first efficiency baseline, at most 150,000,000 tokens.
    S10,
}

impl BudgetStage {
    /// The largest cumulative budget any stage has been approved for.
    pub const MAX_CUMULATIVE: u64 = 150_000_000;

    pub const fn cumulative_cap(self) -> u64 {
        match self {
            Self::S09 => 50_000_000,
            Self::S10 => Self::MAX_CUMULATIVE,
        }
    }

    pub const fn directory(self) -> &'static str {
        match self {
            Self::S09 => "S09",
            Self::S10 => "S10",
        }
    }

    /// Whether a real run's limits stay inside this stage's approval.
    pub const fn admits(self, limits: &Limits) -> bool {
        limits.cumulative > 0 && limits.cumulative <= self.cumulative_cap()
    }
}
