//! Production admission, authorization, dispatch and actual file effects.
use super::*;
mod consent_fixture;
mod control_resources;
mod directory_recovery;
mod directory_scope;
mod editor_publication;
mod external_capabilities;
mod file_boundaries;
mod file_publication;
mod live_file_stops;
mod mode_defaults;
mod native_names;
mod non_recursive_contents;
mod ordinary_reads;
mod permission_changes;
mod prepared_files;
mod workspace_walk;
