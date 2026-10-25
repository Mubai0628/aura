//! Per-capability authorization for dedicated networking and its file effects.
mod authorization;
mod validation;
pub use authorization::{
    authorize, capability_authority, is_ordinary, permitted_now, uses_capability_slices,
};
pub use validation::validate_composite;
