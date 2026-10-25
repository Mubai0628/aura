//! Built-in Shell defaults use compat exclusively; legacy backends remain optional interfaces.
use super::*;
use aura_execution_orchestrator::Selection;
use aura_security_contract::{
    BackendClass, EnforcementGuarantee, PolicyDenyReason, ResolvedCapability,
};
mod authorization;
mod availability;
mod defaults;
mod support;
