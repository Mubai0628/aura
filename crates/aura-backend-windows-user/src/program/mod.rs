//! The program primitive shared by package Conformance and ordinary execution.
//! Core owns authorization, mappings, result validation and Artifact publication.
mod environment;
mod input;
mod process;
mod session;
pub use environment::ProgramEnvironment;
pub use process::{ProgramInput, ProgramIo, ProgramOutput, ProgramSpec, ProgramStop, run};
pub use session::{MAX_SESSION_FRAME_BYTES, ProgramSession, SessionEvent, SessionSpec};

/// No filesystem or network isolation is claimed for author code running as the user.
pub fn guarantees() -> std::collections::BTreeSet<aura_security_contract::EnforcementGuarantee> {
    use aura_security_contract::EnforcementGuarantee;
    [EnforcementGuarantee::ChildProcessControl, EnforcementGuarantee::BoundedOutput].into()
}

#[cfg(test)]
mod tests;
