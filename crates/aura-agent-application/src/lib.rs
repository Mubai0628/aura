//! The Agent application in Core: an Agent turn from submission to completion (context, model
//! steps, tools, compaction, memory maintenance, attachments, continuation) and the run
//! supervisor.
#![forbid(unsafe_code)]

pub mod agent_application;
pub mod agent_supervision;
