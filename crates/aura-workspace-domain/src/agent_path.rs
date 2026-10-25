//! Model-facing file path input.
//!
//! A model may write a workspace path either as a relative logical path
//! (`src/main.rs`, `.`) or as an absolute path that really points inside the
//! session's workspace. Both spellings must mean the same file. The internal
//! [`WorkspacePath`] stays a strict relative-path invariant: absolute input is
//! resolved against the workspace root, canonicalized, and only then converted
//! back to the relative form, so a path can never smuggle a root escape or a
//! second file identity into the operation.
//!
//! The declared workspace root is the final authority for the mapping. A path
//! that exists but resolves outside the root is
//! [`AgentPathError::OutsideWorkspace`], which is an authorization boundary, not
//! a spelling problem: no different spelling of the same path may turn it into
//! access. A path that does not exist yet is still mapped inside the root when
//! its spelling is unambiguous, so a create can target a new file.
use std::{
    fmt,
    path::{Path, PathBuf},
};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use aura_domain_common::{WorkspacePath, WorkspacePathError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentPathError {
    /// The input is not a usable path at all.
    Invalid { detail: String },
    /// The input is a real path that is not inside the session's workspace.
    OutsideWorkspace,
    /// The input is inside the workspace but is not a legal logical path.
    UnsafeComponent { source: WorkspacePathError },
}

impl fmt::Display for AgentPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { detail } => write!(formatter, "invalid file path: {detail}"),
            Self::OutsideWorkspace => formatter.write_str(
                "the path is outside this session's authorized workspace; use a \
                 workspace-relative path, or an absolute path that lies inside the workspace",
            ),
            Self::UnsafeComponent { source } => write!(formatter, "{source}"),
        }
    }
}

impl std::error::Error for AgentPathError {}

fn invalid(detail: impl Into<String>) -> AgentPathError {
    AgentPathError::Invalid { detail: detail.into() }
}

/// Maps model path input onto the workspace-relative invariant.
///
/// `root` must be the workspace root the session is authorized for.
pub fn resolve_agent_path(root: &Path, input: &str) -> Result<WorkspacePath, AgentPathError> {
    if input.trim().is_empty() {
        return Err(invalid("the path must not be empty"));
    }
    if input.contains('\0') {
        return Err(invalid("the path must not contain a NUL character"));
    }
    // A relative spelling keeps the same logical validation, but resolves `.` and
    // `..` lexically first: a model may reasonably write `src/nested/../main.rs`.
    // A `..` that would climb above the root is still refused, because there is no
    // root to anchor it against here.
    if !is_absolute_spelling(input) {
        return workspace_relative(input.trim_matches(['/', '\\']));
    }
    let absolute = absolute_from_normalized(&normalize_text(&strip_verbatim(input)));
    let resolved = resolve_for_containment(&absolute);
    let canonical_root = resolve_for_containment(root);
    let Some(relative) = strip_root(&canonical_root, &resolved) else {
        return Err(AgentPathError::OutsideWorkspace);
    };
    if relative.is_empty() {
        return Ok(WorkspacePath::parse(WorkspacePath::ROOT).expect("root path is valid"));
    }
    WorkspacePath::parse(relative).map_err(|source| AgentPathError::UnsafeComponent { source })
}

/// The path a handle would really open, in normal (non-verbatim) form.
///
/// Symlinks and reparse points are followed so containment is judged by the final
/// target and not by the spelling. A path that does not exist yet cannot be
/// canonicalized, so its lexical form is used; that only ever makes the check
/// stricter, never more permissive, because the lexical form still has to sit
/// under the canonical root.
fn resolve_for_containment(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(value) => strip_verbatim_prefix(value),
        Err(_) => path.to_path_buf(),
    }
}

fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    for prefix in ["\\\\?\\", "\\\\.\\"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            // A verbatim UNC path (`\\?\UNC\server\share`) drops to `\\server`.
            if let Some(unc) = rest.strip_prefix("UNC\\") {
                return PathBuf::from(format!("\\\\{unc}"));
            }
            return PathBuf::from(rest);
        }
    }
    path
}

/// The workspace-relative remainder of `path` under `root`, compared component by
/// component.
///
/// A component comparison is used instead of a string prefix, because two
/// different ancestor directories can share a textual prefix (`C:\work` and
/// `C:\workshop`) and because a component such as a drive letter or a UNC host
/// carries no separator of its own. Only the byte case is folded, because Windows
/// file names and these prefixes are case-insensitive.
fn strip_root(root: &Path, path: &Path) -> Option<String> {
    let mut path_components = path.components();
    for expected in root.components() {
        let actual = path_components.next()?;
        let expected = expected.as_os_str().to_string_lossy().to_lowercase();
        let actual_text = actual.as_os_str().to_string_lossy().to_lowercase();
        if expected != actual_text {
            return None;
        }
    }
    let mut relative = String::new();
    for component in path_components {
        let text = component.as_os_str().to_string_lossy();
        if text.is_empty() || text == "." {
            continue;
        }
        if !relative.is_empty() {
            relative.push('/');
        }
        relative.push_str(&text);
    }
    Some(relative)
}

/// Applies the logical path invariant to an already-rooted relative spelling.
///
/// `.` segments are dropped and `..` segments pop the previous component, so
/// `src/nested/../main.rs` names the same file as `src/main.rs`. A `..` with
/// nothing left to pop is a real escape attempt and is refused.
fn workspace_relative(input: &str) -> Result<WorkspacePath, AgentPathError> {
    let mut parts: Vec<&str> = Vec::new();
    for component in input.split(['/', '\\']) {
        match component {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(AgentPathError::UnsafeComponent {
                        source: WorkspacePathError::ParentTraversal,
                    });
                }
            }
            other => parts.push(other),
        }
    }
    let joined = if parts.is_empty() { WorkspacePath::ROOT.to_owned() } else { parts.join("/") };
    WorkspacePath::parse(joined).map_err(|source| AgentPathError::UnsafeComponent { source })
}

/// Turns a `/`-separated normalized absolute spelling into a real absolute path.
///
/// The string is composed first and only then parsed, because pushing a
/// drive-letter component onto a `PathBuf` yields a drive-*relative* path instead
/// of appending a segment. A leading separator before a drive component is an
/// artifact of separator normalization; before a UNC host it is part of the path.
fn absolute_from_normalized(normalized: &str) -> PathBuf {
    let mut text = String::new();
    for component in normalized.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            match text.rfind('/') {
                Some(index) => text.truncate(index),
                None => text.clear(),
            }
            continue;
        }
        text.push('/');
        text.push_str(component);
    }
    let text = match text.strip_prefix('/') {
        Some(rest) if is_drive_component(rest) => rest.to_owned(),
        _ => text,
    };
    PathBuf::from(text.replace('/', std::path::MAIN_SEPARATOR_STR))
}

/// A path is absolute when it carries a root, a drive prefix, or a UNC form. A
/// leading separator counts as absolute too, because it can only be anchored at
/// the workspace root.
fn is_absolute_spelling(input: &str) -> bool {
    if Path::new(input).is_absolute() {
        return true;
    }
    let normalized = input.replace('\\', "/");
    if normalized.starts_with('/') {
        return true;
    }
    let mut characters = normalized.chars();
    matches!(
        (characters.next(), characters.next()),
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
    )
}

/// Removes a `\\?\` or `\\.\` verbatim prefix so the path can be joined to and
/// compared against a canonical root. A device name that survives this becomes a
/// normal component and is rejected by the logical path validation.
fn strip_verbatim(input: &str) -> String {
    let normalized = input.replace('\\', "/");
    for prefix in ["//?/", "//./"] {
        if normalized.len() >= prefix.len()
            && normalized[..prefix.len()].eq_ignore_ascii_case(prefix)
        {
            // A verbatim UNC path (`//?/UNC/server/share`) becomes `//server/share`.
            let rest = &normalized[prefix.len()..];
            if let Some(unc) = rest.strip_prefix("UNC/").or_else(|| rest.strip_prefix("unc/")) {
                return format!("//{unc}");
            }
            return rest.to_owned();
        }
    }
    normalized
}

/// Collapses `.` and `..` textually. A `..` that would climb above the anchor is
/// dropped, because the canonical root is the authority and the resolved path is
/// compared against it afterwards.
fn normalize_text(text: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for component in text.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("/{}", parts.join("/"))
}

/// True when a normalized path starts with a `C:` style drive component.
fn is_drive_component(text: &str) -> bool {
    let mut characters = text.chars();
    matches!(
        (characters.next(), characters.next()),
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
    )
}

/// The model-facing spelling of a single file path.
///
/// It accepts the relative form on the wire and only ever holds the validated
/// relative value, so every consumer keeps the existing invariants. Core
/// rewrites the operation's path arguments through [`resolve_agent_path`] while
/// it still holds the authorized workspace root, which is why an absolute
/// spelling is refused at this boundary: accepting one here would let a caller
/// fabricate a logical path without any root to resolve it against.
#[derive(Clone, Debug, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd)]
#[schemars(
    with = "String",
    description = "A path relative to the workspace root (\".\" is the root). An absolute path inside the workspace names the same file."
)]
pub struct AgentWorkspacePath(pub WorkspacePath);

impl AgentWorkspacePath {
    /// The workspace root, the default directory of the search and find tools.
    #[must_use]
    pub fn root() -> Self {
        Self(WorkspacePath::root())
    }

    #[must_use]
    pub fn relative(&self) -> &WorkspacePath {
        &self.0
    }

    #[must_use]
    pub fn into_relative(self) -> WorkspacePath {
        self.0
    }
}

impl From<WorkspacePath> for AgentWorkspacePath {
    fn from(value: WorkspacePath) -> Self {
        Self(value)
    }
}

impl fmt::Display for AgentWorkspacePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

impl Serialize for AgentWorkspacePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for AgentWorkspacePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if is_absolute_spelling(&value) {
            return Err(D::Error::custom(
                "workspace path must be normalized to a workspace-relative path before this point",
            ));
        }
        WorkspacePath::parse(value).map(Self).map_err(|error| D::Error::custom(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{AgentPathError, resolve_agent_path};
    use aura_domain_common::WorkspacePath;

    fn workspace() -> (tempfile::TempDir, PathBuf) {
        let workspace = tempfile::tempdir().expect("workspace root");
        std::fs::create_dir_all(workspace.path().join("src/nested")).expect("src");
        std::fs::write(workspace.path().join("src/main.rs"), "fn main() {}").expect("main");
        let root = workspace.path().canonicalize().expect("canonical root");
        (workspace, root)
    }

    #[test]
    fn relative_and_absolute_spellings_of_one_file_agree() {
        let (_workspace, root) = workspace();
        let absolute = root.join("src").join("main.rs");
        let from_relative =
            resolve_agent_path(&root, "src/main.rs").expect("relative path resolves");
        let from_absolute = resolve_agent_path(&root, &absolute.display().to_string())
            .unwrap_or_else(|error| panic!("absolute path resolves: {error}"));
        assert_eq!(from_relative, from_absolute);
        assert_eq!(from_relative.as_str(), "src/main.rs");
        let slashed = absolute.display().to_string().replace('\\', "/");
        assert_eq!(resolve_agent_path(&root, &slashed).expect("slashed"), from_relative);
    }

    #[test]
    fn separators_dot_segments_and_the_root_itself_are_equivalent() {
        let (_workspace, root) = workspace();
        let expected = WorkspacePath::parse("src/main.rs").expect("path");
        for spelling in
            [r"src\main.rs", "./src/main.rs", "src/nested/../main.rs", r"src\nested\..\main.rs"]
        {
            assert_eq!(
                resolve_agent_path(&root, spelling).expect("equivalent spelling"),
                expected,
                "{spelling} must resolve to the same file"
            );
        }
        // A bare `/` is anchored at the current drive root, which is deliberately
        // not the same thing as a specific workspace, so it is not listed here.
        for root_spelling in [".", &root.display().to_string()] {
            assert_eq!(
                resolve_agent_path(&root, root_spelling)
                    .unwrap_or_else(|error| panic!("{root_spelling}: {error}")),
                WorkspacePath::parse(WorkspacePath::ROOT).expect("root path"),
                "{root_spelling} must mean the workspace root"
            );
        }
        assert!(resolve_agent_path(&root, "").is_err());
    }

    #[test]
    fn a_path_outside_the_workspace_is_refused_in_every_spelling() {
        let (workspace, root) = workspace();
        let outside = tempfile::tempdir().expect("outside root");
        std::fs::write(outside.path().join("secret.txt"), "outside").expect("secret");
        let sibling = workspace.path().with_extension("sibling");
        std::fs::create_dir_all(&sibling).expect("sibling");
        std::fs::write(sibling.join("note.txt"), "sibling").expect("note");

        for spelling in [
            outside.path().join("secret.txt").display().to_string(),
            outside.path().join("secret.txt").display().to_string().replace('\\', "/"),
            sibling.join("note.txt").display().to_string(),
            format!("{}\\..\\outside.txt", root.display()),
        ] {
            assert_eq!(
                resolve_agent_path(&root, &spelling),
                Err(AgentPathError::OutsideWorkspace),
                "{spelling} must be refused as outside the workspace"
            );
        }

        // A sibling directory whose name merely starts with the root's name must
        // not be treated as inside it: containment is decided per path component,
        // never by a string prefix.
        let lookalike = PathBuf::from(format!("{}x", root.display()));
        std::fs::create_dir_all(&lookalike).expect("lookalike");
        std::fs::write(lookalike.join("note.txt"), "lookalike").expect("lookalike note");
        assert_eq!(
            resolve_agent_path(&root, &lookalike.join("note.txt").display().to_string()),
            Err(AgentPathError::OutsideWorkspace),
            "a prefix-sharing sibling directory is still outside the workspace"
        );
    }

    #[test]
    fn a_missing_file_inside_the_root_still_maps_and_an_unsafe_name_is_reported() {
        let (_workspace, root) = workspace();
        assert_eq!(
            resolve_agent_path(&root, &root.join("src/new.rs").display().to_string())
                .expect("a create target inside the root is representable"),
            WorkspacePath::parse("src/new.rs").expect("path")
        );
        assert!(matches!(
            resolve_agent_path(&root, "src/CON.txt"),
            Err(AgentPathError::UnsafeComponent { .. })
        ));
        assert!(matches!(
            resolve_agent_path(&root, "src/evil.txt:stream"),
            Err(AgentPathError::UnsafeComponent { .. })
        ));
        // A NUL byte cannot appear in a real Windows path at all.
        assert!(matches!(
            resolve_agent_path(&root, "src/nu\u{0}l.rs"),
            Err(AgentPathError::Invalid { .. })
        ));
        // A relative spelling that climbs above the root has nothing to anchor it.
        assert!(matches!(
            resolve_agent_path(&root, "../escape.txt"),
            Err(AgentPathError::UnsafeComponent { .. })
        ));
    }

    /// A relative-path spelling is pure string work, so it is exercised here even
    /// where this environment cannot create the reparse point the link test needs.
    #[test]
    fn relative_traversal_out_of_the_root_is_refused() {
        let (_workspace, root) = workspace();
        for spelling in ["../escape.txt", "src/../../escape.txt", r"src\..\..\escape.txt"] {
            assert!(
                matches!(
                    resolve_agent_path(&root, spelling),
                    Err(AgentPathError::UnsafeComponent { .. })
                ),
                "{spelling} climbs above the root and must be refused"
            );
        }
    }

    /// A junction is judged by the final path its handle reaches, not by the
    /// spelling that names it. Creating a reparse point needs a privilege this
    /// environment may not grant, in which case the check is skipped and the
    /// production resolver's own junction policy remains the authority.
    #[test]
    fn a_link_that_escapes_the_root_is_refused_by_its_final_path() {
        let (workspace, root) = workspace();
        let outside = tempfile::tempdir().expect("outside root");
        std::fs::write(outside.path().join("secret.txt"), "outside").expect("secret");
        let link = workspace.path().join("escape");
        if junction::create(outside.path(), &link).is_err() {
            return;
        }
        let reached = std::fs::canonicalize(link.join("secret.txt"))
            .expect("the junction target is reachable through the link");
        assert!(
            !reached.starts_with(&root),
            "the link really reaches outside the root: {}",
            reached.display()
        );
        assert_eq!(
            resolve_agent_path(&root, &root.join("escape/secret.txt").display().to_string()),
            Err(AgentPathError::OutsideWorkspace),
            "an absolute path through the link is judged by its final target"
        );
    }
}
