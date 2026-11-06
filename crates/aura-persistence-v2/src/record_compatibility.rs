//! Which versions can read the records this build writes. Before this build first starts in place
//! of an older version, the launcher copies every store that version could not read afterwards,
//! so that going back puts the copy back instead of leaving it records it refuses.
use std::path::Path;

use aura_persistence_core::StoreResult;

/// The oldest version that reads every record this build writes. 0.1.12 and earlier refuse an
/// authorization scope naming a desktop program's consent, request options with a speed mode,
/// and queued messages with references or a desktop choice. Raised to a new version whenever it
/// starts storing a field or an enum value its predecessor refuses.
pub const RECORDS_READABLE_SINCE: &str = "0.2.0";

/// Whether the execution store at `path` needs a copy before this build first starts in place of
/// `replacing` (unknown: any older version): this build migrates it, or `replacing` would refuse
/// what this build writes into it.
pub fn copy_before_start(path: &Path, replacing: Option<&str>) -> StoreResult<bool> {
    Ok(crate::migration_pending(path)? || (path.is_file() && refuses_records(replacing)))
}

fn refuses_records(replacing: Option<&str>) -> bool {
    let parts = |version: &str| -> Vec<u64> {
        version.split('.').map(|part| part.parse().unwrap_or(0)).collect()
    };
    replacing.is_none_or(|version| parts(version) < parts(RECORDS_READABLE_SINCE))
}

#[cfg(test)]
mod tests {
    use super::refuses_records;

    #[test]
    fn versions_compare_by_number_and_an_unknown_one_refuses() {
        assert!(refuses_records(Some("0.1.7")));
        assert!(refuses_records(Some("0.0.9")));
        assert!(refuses_records(None));
        assert!(refuses_records(Some("0.1.12")), "0.1.12 refuses what 0.2.0 stores");
        assert!(!refuses_records(Some("0.2.0")));
        assert!(!refuses_records(Some("0.10.0")));
        assert!(!refuses_records(Some("1.0.0")));
    }
}
