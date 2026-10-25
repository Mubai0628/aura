//! Hooks: the package format, the events Aura offers, and the one result protocol.
//!
//! A Hook is the user's own program. It runs only after the user trusted the exact content
//! (the package fingerprint), it can only continue, transform within the event's boundary or
//! block, and nothing it returns can approve, grant, ask the user or skip an audit.
mod event;
mod manifest;
mod package;
mod protocol;
mod views;

pub use event::HookEvent;
pub use manifest::{
    HOOK_MANIFEST_FILE, HookCommand, HookDefinition, HookFailurePolicy, HookKind, parse_manifest,
};
pub use package::{
    HookPackage, HookPackageError, HookPackageFile, MAX_PACKAGE_BYTES, MAX_PACKAGE_FILES,
    scan_package,
};
pub use protocol::{
    HOOK_PROTOCOL, HookDecision, HookEnvelope, HookEnvelopeHook, HookModelParameters,
    HookResultError, HookTransform, MAX_RESULT_BYTES, parse_result,
};
pub use views::*;
