//! The P04 publication's activity records, next to the shared ones of
//! `aura_core_activity::agent_activity`, which are mounted here under their usual names.
pub(crate) use aura_core_activity::agent_activity::*;
mod p04;
pub(crate) use p04::{p04_file_observer, publish_p04_publication_activity};
