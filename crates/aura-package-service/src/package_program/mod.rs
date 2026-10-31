//! Package program composition for both Conformance and Agent dispatch. This
//! module consumes Core-resolved inputs; it grants no execution permission.
//! Callers must hold their exact user approval/start authority through launch.
pub mod claims;
mod conformance;
mod execute;
pub mod media_type;
mod outputs;
mod protocol;
pub use conformance::{PackageConformanceRequest, ProgramCaseObservation, run_conformance};
pub use execute::{
    ProgramContext, ProgramInvocationIdentity, ProgramRunResult, program_declares_network, run_case,
};
pub use outputs::VerifiedProgramArtifact;

#[cfg(any(test, feature = "test-hooks"))]
pub mod tests;
