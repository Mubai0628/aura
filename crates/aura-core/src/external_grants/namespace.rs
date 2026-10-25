//! Relative keys compare native names under each actual parent directory's rule.
use super::*;

pub(super) fn relative_under(root: &Path, path: &Path) -> Option<WorkspacePath> {
    use std::path::Component;
    let mut components = path.components();
    let mut parent = PathBuf::new();
    for expected in root.components() {
        let actual = components.next()?;
        let same = match (expected, actual) {
            (Component::Prefix(left), Component::Prefix(right)) => left
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy()),
            (Component::RootDir, Component::RootDir) => true,
            (Component::Normal(left), Component::Normal(right)) => {
                let directory = aura_windows_platform::open_verified_existing(
                    &parent,
                    NativeFileAccess::Metadata,
                )
                .ok()?;
                match aura_windows_platform::directory_case_rule(&directory) {
                    aura_windows_platform::VolumeCaseRule::Sensitive => left == right,
                    aura_windows_platform::VolumeCaseRule::InsensitivePreserving => {
                        let map = aura_windows_platform::directory_name_comparison(&directory)?;
                        map.key(&left.to_string_lossy()) == map.key(&right.to_string_lossy())
                    }
                    aura_windows_platform::VolumeCaseRule::Unknown => return None,
                }
            }
            _ => false,
        };
        if !same {
            return None;
        }
        parent.push(actual.as_os_str());
    }
    join_relative(components).ok()
}
