//! The Agent store's tests that need a started Core. They read the store's names through
//! `use super::super::*`, as they did inside its module; these imports give them the same names.
use aura_core_broker::agent_store::*;
use aura_persistence_core::{Connection, PersistenceCoreError};
use std::sync::Arc;

mod tests;
