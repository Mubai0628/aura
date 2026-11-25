//! Session collaboration owns its data; the application supplies host capabilities.
pub use aura_agent_collaboration_contract::*;
pub mod dispatch;
pub mod graph;
pub mod maintenance;
pub mod policy;
pub mod profile;
pub const SCHEMA: &str = include_str!("schema.sql");
