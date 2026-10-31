//! Normalizes the model's file path spelling before an operation is built.
//!
//! The model may name a file relatively (`src/main.rs`) or absolutely
//! (`C:\work\aura\src\main.rs`). Both must mean the same file, and both must be
//! judged against the session's **declared workspace root**, never by comparing
//! strings. This module is the single place that maps the spelling onto the
//! `WorkspacePath` invariant, so every `fs.*` handler sees one canonical form and
//! the operation fingerprint, resource binding and change conflict checks cannot
//! be bypassed by respelling the same file.
use aura_workspace_domain::{AgentPathError, resolve_agent_path};
use serde_json::{Map, Value};

/// Path-bearing arguments of a model file tool. Each name is normalized in
/// place; an argument that is absent is left absent so the handler's own schema
/// validation still reports a missing field as a missing field.
pub(crate) const PATH_FIELDS: &[&str] = &["path"];
pub(crate) const ROOT_FIELDS: &[&str] = &["root"];

pub(crate) fn normalize_action_paths(
    action: &mut Value,
    fields: &[&str],
    workspace_root: &std::path::Path,
) -> anyhow::Result<()> {
    let Some(object) = action.as_object_mut() else {
        anyhow::bail!("agent.path_argument_not_an_object");
    };
    for field in fields {
        let Some(value) = object.get_mut(*field) else { continue };
        let Some(text) = value.as_str() else {
            anyhow::bail!("agent.path_argument_must_be_a_string:{field}");
        };
        let normalized =
            resolve_agent_path(workspace_root, text).map_err(|error| path_error(field, error))?;
        *value = Value::String(normalized.as_str().to_owned());
    }
    Ok(())
}

/// The field of the one path failure an authorized external target may answer: the
/// spelling names a real path, and it is not inside the workspace. Every other
/// failure — an unusable spelling, an unsafe component, a climb above the root —
/// is a spelling problem and stays one.
pub(crate) fn outside_workspace_field(error: &anyhow::Error) -> Option<String> {
    error
        .to_string()
        .strip_prefix(OUTSIDE_WORKSPACE_PREFIX)
        .and_then(|rest| rest.split(':').next())
        .filter(|field| !field.is_empty())
        .map(str::to_owned)
}

const OUTSIDE_WORKSPACE_PREFIX: &str = "agent.path_outside_workspace:";

/// Turns one path failure into the message the model receives. The class and the
/// field are both part of the message, because the code is the only channel the
/// model sees: a path it spelled wrongly, a path that is unsafe, and a path outside
/// its authorized scope need different corrections.
fn path_error(field: &str, error: AgentPathError) -> anyhow::Error {
    match error {
        AgentPathError::Invalid { detail } => {
            anyhow::anyhow!("agent.path_invalid:{field}:{detail}")
        }
        AgentPathError::OutsideWorkspace => anyhow::anyhow!(
            "{OUTSIDE_WORKSPACE_PREFIX}{field}: the path is outside this session's authorized \
             workspace"
        ),
        AgentPathError::UnsafeComponent { source } => {
            anyhow::anyhow!("agent.path_unsafe_component:{field}:{source}")
        }
    }
}

/// Rewrites the path-bearing fields of a nested cursor object so a paged request
/// keeps the same canonical relative spelling as the first page. A cursor that is
/// absent or null is left untouched, and a cursor that is not an object is left
/// for the handler's own schema validation to reject.
pub(crate) fn normalize_cursor_paths(
    object: &mut Map<String, Value>,
    field: &str,
    path_fields: &[&str],
    workspace_root: &std::path::Path,
) -> anyhow::Result<()> {
    let Some(cursor @ Value::Object(_)) = object.get_mut(field) else { return Ok(()) };
    normalize_action_paths(cursor, path_fields, workspace_root)
}
