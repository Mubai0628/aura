//! Real Core mode authority and SQLite journal; descriptor is a unit fixture,
//! not evidence that an installed account or Runner has passed Host acceptance.
#![allow(clippy::unwrap_used)]
use super::background_observations;
use super::port::CorePort;
use aura_backend_windows_user::WindowsUserCorePort;
use aura_domain_common::*;
use aura_execution_contract::*;
use aura_security_contract::*;
use aura_windows_user_contract::*;
use std::sync::Arc;
#[path = "../activity_observation_tests.rs"]
mod activity_observation_tests;
mod permission_changes;

// Behavior tests are real child modules; shared fixtures are imported explicitly.
mod authority;
mod launch;
mod output;
mod support;
mod terminal_input;
mod terminal_opening;
mod terminal_shutdown;
mod user_terminal;

use support::*;
