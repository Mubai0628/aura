//! A store an older version wrote keeps, beside what is read again, copies nothing reads. Slimming
//! clears them once, in batches a later run can continue, and rebuilds the file when the disk has
//! room for it.
#![allow(clippy::unwrap_used)]
#[path = "support/release_0_1_9.rs"]
mod release_0_1_9;
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

mod store_slimming {
    mod cleanup;
    mod old_store;
    mod owed_on_open;
    mod rebuild;
    mod resume;
    mod streams;
}
