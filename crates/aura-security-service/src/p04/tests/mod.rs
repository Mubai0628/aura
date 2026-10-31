use super::*;

// Behavior tests are real child modules; shared fixtures are imported explicitly.
mod approval_defaults;
mod host_recovery;
mod main_authority;
mod refusals;

use super::host_begin::host_preparation_approval_view;
use super::host_recovery::{InterruptedHostFacts, classify_interrupted_from_facts};
