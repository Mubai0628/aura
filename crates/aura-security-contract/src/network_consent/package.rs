//! The trusted package tool exception. A package tool runs only after the user
//! installed and trusted its exact version; a session answer covers that version only.
use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How a package tool reaches the outside: through Core's controlled HTTP, or as a
/// program running with the user's identity in a Job.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PackageRuntimeClass {
    Http,
    Program,
}

/// The package a call names: exactly one `ToolPackage` resource, a non-built-in tool
/// source and a package tool name (`user.<package>.<tool>`). Anything else is not a
/// package call and borrows nothing from this exception.
#[must_use]
pub fn package_tool_target<'a>(
    source: &SecuritySourceRef,
    action: &str,
    resources: &'a [ResourceDescriptor],
) -> Option<(
    &'a aura_domain_common::ToolPackageId,
    &'a aura_domain_common::jcs::CanonicalSha256HashV1,
    PackageRuntimeClass,
)> {
    if !matches!(source, SecuritySourceRef::Tool { .. }) || !action.starts_with("user.") {
        return None;
    }
    let mut found = resources.iter().filter_map(|resource| match resource {
        ResourceDescriptor::ToolPackage { package_id, package_fingerprint, runtime } => {
            Some((package_id, package_fingerprint, *runtime))
        }
        _ => None,
    });
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

/// A trusted package program call: the exact capability shape Core gives one. A call
/// with any other capability is not covered by the package exception.
#[must_use]
pub fn package_program(
    source: &SecuritySourceRef,
    action: &str,
    caps: &[ResolvedCapability],
    resources: &[ResourceDescriptor],
) -> bool {
    package_tool_target(source, action, resources)
        .is_some_and(|(_, _, runtime)| runtime == PackageRuntimeClass::Program)
        && caps.contains(&ResolvedCapability::ProcessExecute)
        && caps.iter().all(|cap| {
            matches!(cap, ResolvedCapability::ProcessExecute | ResolvedCapability::OrdinaryNetwork)
        })
}

/// Whether this operation's network is Core-controlled exact-origin HTTP: the
/// built-in ordinary tools, or a package HTTP tool in its exact shape. A package
/// program's network is the program's own and is never controlled.
#[must_use]
pub fn controlled_tool_network(
    source: &SecuritySourceRef,
    action: &str,
    caps: &[ResolvedCapability],
    resources: &[ResourceDescriptor],
) -> bool {
    ordinary_tool_network_caller(source, action) == Some(NetworkConsentCaller::OrdinaryTools)
        || package_http(source, action, caps, resources)
}

/// A package HTTP call: Core's controlled HTTP, one exact origin, ordinary network only.
#[must_use]
pub fn package_http(
    source: &SecuritySourceRef,
    action: &str,
    caps: &[ResolvedCapability],
    resources: &[ResourceDescriptor],
) -> bool {
    package_tool_target(source, action, resources)
        .is_some_and(|(_, _, runtime)| runtime == PackageRuntimeClass::Http)
        && caps == [ResolvedCapability::OrdinaryNetwork]
}
