use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileIdentity {
    pub volume_serial_number: String,
    pub file_id: String,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum WorkspacePathError {
    #[error("workspace path must not be empty")]
    Empty,
    #[error("workspace path must be relative")]
    Absolute,
    #[error("workspace path contains a parent traversal")]
    ParentTraversal,
    #[error("workspace path contains a device, named-pipe, ADS, or NUL form")]
    DeviceOrAlternateStream,
    #[error("workspace path contains an ambiguous Windows component: {component}")]
    AmbiguousComponent { component: String },
}

#[derive(Clone, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd)]
#[schemars(with = "String")]
pub struct WorkspacePath(String);

impl WorkspacePath {
    pub const ROOT: &'static str = ".";

    /// The workspace root itself.
    #[must_use]
    pub fn root() -> Self {
        Self(Self::ROOT.to_owned())
    }

    pub fn parse(value: impl AsRef<str>) -> Result<Self, WorkspacePathError> {
        let value = value.as_ref();
        if value.is_empty() {
            return Err(WorkspacePathError::Empty);
        }
        if value.contains('\0') {
            return Err(WorkspacePathError::DeviceOrAlternateStream);
        }
        let lower = value.to_ascii_lowercase();
        if value.starts_with('/')
            || value.starts_with('\\')
            || lower.starts_with("\\\\?\\")
            || lower.starts_with("\\\\.\\")
            || lower.starts_with("//?/")
            || lower.starts_with("//./")
            || lower.starts_with("\\\\pipe\\")
        {
            return Err(WorkspacePathError::Absolute);
        }
        let mut components = Vec::new();
        for component in value.split(['/', '\\']) {
            if component.is_empty() || component == "." {
                continue;
            }
            if component == ".." {
                return Err(WorkspacePathError::ParentTraversal);
            }
            if component.contains(':') {
                return Err(WorkspacePathError::DeviceOrAlternateStream);
            }
            if component.ends_with([' ', '.']) || is_reserved_windows_name(component) {
                return Err(WorkspacePathError::AmbiguousComponent {
                    component: component.to_owned(),
                });
            }
            components.push(component);
        }
        if components.is_empty() {
            Ok(Self(Self::ROOT.to_owned()))
        } else {
            Ok(Self(components.join("/")))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_root(&self) -> bool {
        self.0 == Self::ROOT
    }
}

impl fmt::Debug for WorkspacePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("WorkspacePath").field(&self.0).finish()
    }
}

impl fmt::Display for WorkspacePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for WorkspacePath {
    type Err = WorkspacePathError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for WorkspacePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for WorkspacePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

fn is_reserved_windows_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component).trim_end_matches([' ', '.']);
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || upper.strip_prefix("COM").or_else(|| upper.strip_prefix("LPT")).is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
}

#[cfg(test)]
mod tests {
    use super::WorkspacePath;

    #[test]
    fn workspace_paths_normalize_separators_and_reject_escapes() {
        assert_eq!(WorkspacePath::parse(r"src\\main.rs").expect("path").as_str(), "src/main.rs");
        assert!(WorkspacePath::parse(r"..\\secret.txt").is_err());
        assert!(WorkspacePath::parse(r"C:\\secret.txt").is_err());
        assert!(WorkspacePath::parse(r"file.txt:stream").is_err());
        assert!(WorkspacePath::parse(r"CON.txt").is_err());
    }
}
