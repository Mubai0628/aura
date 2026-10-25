//! Native directory-name comparison. Verbatim prefixes are transport syntax only.
use super::*;
use aura_windows_platform::{NativeFileAccess, open_verified_existing};

pub(super) fn within(candidate: &str, root: &str) -> bool {
    if root.is_empty() {
        return false;
    }
    let root_path = Path::new(root);
    let candidate_path = Path::new(candidate);
    if let Ok(root_info) = aura_windows_platform::inspect_existing_path(root_path, true) {
        let mut current = Some(candidate_path);
        while let Some(path) = current {
            match aura_windows_platform::inspect_existing_path(path, true) {
                Ok(info) => {
                    return crate::agent_execution::file_targets::native_scope::within(
                        &root_info, &info,
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    current = path.parent()
                }
                Err(_) => break,
            }
        }
    }
    // A protected entry not yet created is compared under its real parent's name rule.
    let roots = root.trim_end_matches('\\').split('\\').collect::<Vec<_>>();
    let candidates = candidate.trim_end_matches('\\').split('\\').collect::<Vec<_>>();
    if candidates.len() < roots.len() {
        return false;
    }
    let mut parent = PathBuf::new();
    for (index, (expected, actual)) in roots.iter().zip(candidates.iter()).enumerate() {
        let same = if index == 0 {
            expected.eq_ignore_ascii_case(actual)
        } else {
            match open_verified_existing(&parent, NativeFileAccess::Metadata) {
                Ok(directory) => match aura_windows_platform::directory_case_rule(&directory) {
                    aura_windows_platform::VolumeCaseRule::Sensitive => expected == actual,
                    aura_windows_platform::VolumeCaseRule::InsensitivePreserving => {
                        aura_windows_platform::directory_name_comparison(&directory)
                            .is_some_and(|map| map.key(expected) == map.key(actual))
                    }
                    aura_windows_platform::VolumeCaseRule::Unknown => expected == actual,
                },
                Err(_) => expected == actual,
            }
        };
        if !same {
            return false;
        }
        if index == 0 {
            parent = PathBuf::from(format!("{actual}\\"));
        } else {
            parent.push(actual);
        }
    }
    true
}
