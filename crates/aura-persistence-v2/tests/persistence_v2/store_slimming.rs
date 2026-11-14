//! A store an older version wrote keeps, beside what is read again, copies nothing reads. Slimming
//! clears them once, in batches a later run can continue, and rebuilds the file when the disk has
//! room for it.
#![allow(clippy::unwrap_used)]
use crate::release_0_1_9;
use crate::support;

mod cleanup;
mod old_store;
mod owed_on_open;
mod rebuild;
mod resume;
mod streams;
