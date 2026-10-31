//! Third repair, batch 1: the recovery **lifecycle** — Models-only recovery, combined faults,
//! and the first recovery a process never performed.
//!
//! # What these cases are for
//!
//! Repairing `tools.json` brings the tool-policy chain back, but that is one shape of fault, not
//! the general case. These cases close three gaps:
//!
//! 1. **`Models` is not a leaf.** A damaged `models.json` at startup means no `P05Services`, and
//!    `open_dependent_services` builds the Agent application only from `(commands, p05)` — so the
//!    Agent application was never created either. Supplying P05 alone left the process with an
//!    authority and no consumer, and the ordinary Agent call that reads it still failed.
//! 2. **The first recovery may never have happened.** With no P05 there is no maintenance task; with
//!    no Agent application there is no inbox scan, no `restore_waits` and no derived backfill. The
//!    live constructors assumed that startup work had been done. A marker that is merely *set* is not
//!    evidence, so the process keeps a real ledger of what finished.
//! 3. **Health and real availability are different facts.** `agent_work_available` read only the
//!    degradation record, so a capability whose service could not be built was reported available as
//!    soon as the record was cleared.
//!
//! # How the damage is real
//!
//! Each case replaces a domain's file with a **directory** of the same name: the path still exists,
//! so the facade believes there is a file, and the real `File::open` fails with a genuine Windows
//! I/O error. Nothing is simulated and no in-memory snapshot stands in for a real read.
use super::*;
use crate::r03_tests::{Fixture, success};
use aura_config::{ConfigModule, SettingsFacade};
use aura_core_base::config_degradation::{ConfigDegradationRegistry, ConfigDependentCapability};
use serde_json::json;

// Behavior tests are real child modules; shared fixtures are imported explicitly.
mod admission;
mod dependent_chain;
mod startup;
mod support;

use support::*;
