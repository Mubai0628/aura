use crate::ActivityResourceRelation;

impl ActivityResourceRelation {
    /// Whether the write set behind this relation cannot be proven, wherever it lies.
    #[must_use]
    pub const fn is_unknown_write_set(self) -> bool {
        matches!(self, Self::UnknownWriteSet | Self::UnknownWriteSetOutsideWorkspace)
    }
}
