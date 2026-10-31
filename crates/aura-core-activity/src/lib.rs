//! What Core's services publish about the work they are really doing: the registry of active
//! operations that cancellation reaches (`activity`), and the records real executors publish
//! into the run-information index, with the queries over that index (`agent_activity`).
#![forbid(unsafe_code)]

pub mod activity;
pub mod agent_activity;
