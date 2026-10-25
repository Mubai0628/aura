#![forbid(unsafe_code)]
//! Core's backend-neutral Tool execution orchestration. Admission/planning do
//! not acquire locks or create attempts. Stores and native execution stay in
//! their owning adapters; this crate has no SQL, filesystem or network client.
mod admission;
mod batch;
mod dispatch;
pub use admission::*;
pub use batch::*;
pub use dispatch::*;
