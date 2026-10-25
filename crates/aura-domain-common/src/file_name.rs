//! Immutable, non-linguistic UTF-16 name comparison facts supplied by a filesystem boundary.
//! This primitive performs no I/O and makes no assumption about a filesystem or a locale.

use std::{fmt, sync::Arc};

/// One-to-one uppercase table: no expansion, Unicode normalization, or scalar case folding.
/// Surrogate code units retain their identity, so valid UTF-16 remains valid UTF-16.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct FileNameComparison(Arc<[u16]>);

impl FileNameComparison {
    /// Reject incomplete, non-idempotent, or UTF-16-damaging tables at the boundary.
    #[must_use]
    pub fn from_uppercase_table(table: Vec<u16>) -> Option<Self> {
        if table.len() != 65_536
            || table.iter().enumerate().any(|(unit, mapped)| {
                table[usize::from(*mapped)] != *mapped
                    || (0xd800..=0xdfff).contains(&unit) && usize::from(*mapped) != unit
                    || !(0xd800..=0xdfff).contains(&unit) && (0xd800..=0xdfff).contains(mapped)
            })
        {
            return None;
        }
        Some(Self(table.into()))
    }

    #[must_use]
    pub fn key(&self, name: &str) -> String {
        let units = name.encode_utf16().map(|unit| self.0[usize::from(unit)]).collect::<Vec<_>>();
        String::from_utf16(&units).expect("the validated table preserves UTF-16")
    }
}

impl fmt::Debug for FileNameComparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileNameComparison").field("utf16_units", &self.0.len()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<u16> {
        (0..=u16::MAX).collect()
    }

    #[test]
    fn a_comparison_table_cannot_expand_or_damage_a_name() {
        let mut units = table();
        units[usize::from(b'a')] = u16::from(b'A');
        let comparison = FileNameComparison::from_uppercase_table(units).expect("one-to-one");
        assert_eq!(comparison.key("aßﬃ\u{10428}"), "Aßﬃ\u{10428}");
        assert_ne!(comparison.key("ß"), comparison.key("ss"));
    }

    #[test]
    fn incomplete_non_idempotent_and_surrogate_damaging_tables_are_rejected() {
        assert!(FileNameComparison::from_uppercase_table(vec![0]).is_none());
        let mut units = table();
        units[1] = 2;
        units[2] = 3;
        assert!(FileNameComparison::from_uppercase_table(units).is_none());
        let mut units = table();
        units[0xd800] = 0;
        assert!(FileNameComparison::from_uppercase_table(units).is_none());
    }
}
