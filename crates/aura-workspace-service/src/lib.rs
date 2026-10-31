//! The workspace files service in Core: workspace lifecycle, file reads and writes, buffers and
//! the editor; and what it shares with the security service about workspace mutations.
#![forbid(unsafe_code)]

pub mod p03;
pub mod workspace_mutation;

/// Which Main methods mutate a workspace; p03 asks it before it observes a file read.
pub use workspace_mutation::gates_authenticated_main_workspace_mutation;
