//! The built-in browser's operations.
//!
//! A browser tool acts on one page of the session: `BrowserPage` names it and `BrowserSite` the
//! site it is on (none for a blank or workspace page), with the user's rule when the browser
//! settings allow that site for a ReadOnly session. A navigation to a new site also carries the
//! target as a `NetworkOrigin`. What the session mode covers on its own:
//!
//! - reading the page (snapshot, screenshot, console, waiting, history) — every mode;
//! - opening a site — WorkspaceReadWrite and FullAccess; ReadOnly only for an allowed site;
//! - acting on the page (click, type, keys, select) — WorkspaceReadWrite and FullAccess;
//! - a sensitive action — never: it asks every time.
//!
//! Everything else asks, and a ReadOnly answer may cover the site for the session. Ordinary network
//! approval never covers the browser, and a browser answer covers nothing else.
use crate::{
    NetworkCallerKind, PermissionMode, ResolvedCapability, ResourceDescriptor, SecuritySourceRef,
};

/// A built-in `browser.*` tool, whatever its shape.
#[must_use]
pub fn browser_tool(source: &SecuritySourceRef, action: &str) -> bool {
    matches!(source, SecuritySourceRef::BuiltInTool { .. }) && action.starts_with("browser.")
}

/// The site a browser operation acts on, and the user's rule that allows it, if any.
#[must_use]
pub fn browser_site(resources: &[ResourceDescriptor]) -> Option<(&str, Option<&str>)> {
    resources.iter().find_map(|resource| match resource {
        ResourceDescriptor::BrowserSite { host, allowed_by_rule } => {
            Some((host.as_str(), allowed_by_rule.as_deref()))
        }
        _ => None,
    })
}

/// Whether the operation opens a site (it names the target).
#[must_use]
pub fn browser_navigation(resources: &[ResourceDescriptor]) -> bool {
    resources.iter().any(|resource| matches!(resource, ResourceDescriptor::NetworkOrigin { .. }))
}

/// A built-in browser tool's operation in its exact shape: browser capabilities (and a workspace
/// file read to open one), one page, at most one site, and a navigation target only together with
/// the site it names.
#[must_use]
pub fn built_in_browser(
    source: &SecuritySourceRef,
    action: &str,
    capabilities: &[ResolvedCapability],
    resources: &[ResourceDescriptor],
) -> bool {
    use ResolvedCapability as C;
    let count =
        |pick: fn(&ResourceDescriptor) -> bool| resources.iter().filter(|r| pick(r)).count();
    let targets: Vec<_> = resources
        .iter()
        .filter_map(|resource| match resource {
            ResourceDescriptor::NetworkOrigin { host, .. } => Some(host.as_str()),
            _ => None,
        })
        .collect();
    browser_tool(source, action)
        && !capabilities.is_empty()
        && capabilities.iter().all(|capability| {
            matches!(
                capability,
                C::BrowserRead
                    | C::BrowserInteraction
                    | C::BrowserSensitiveAction
                    | C::WorkspaceFileRead
            )
        })
        && count(|r| matches!(r, ResourceDescriptor::BrowserPage { .. })) == 1
        && count(|r| matches!(r, ResourceDescriptor::BrowserSite { .. })) <= 1
        && resources.iter().all(|r| {
            matches!(
                r,
                ResourceDescriptor::BrowserPage { .. }
                    | ResourceDescriptor::BrowserSite { .. }
                    | ResourceDescriptor::NetworkOrigin { .. }
                    | ResourceDescriptor::WorkspacePath { .. }
            )
        })
        && targets.len() <= 1
        && targets
            .first()
            .is_none_or(|target| browser_site(resources).is_some_and(|(site, _)| site == *target))
}

/// Which caller a tool's network belongs to. Core derives it from the tool, never from a model.
#[must_use]
pub fn network_caller_for(action: &str) -> NetworkCallerKind {
    if action.starts_with("web.") {
        NetworkCallerKind::WebRuntime
    } else if action.starts_with("browser.") {
        NetworkCallerKind::AuraBrowser
    } else {
        NetworkCallerKind::AuraTool
    }
}

/// What the session mode covers on its own for one capability of a browser operation.
#[must_use]
pub fn browser_mode_covers(
    mode: PermissionMode,
    capability: ResolvedCapability,
    resources: &[ResourceDescriptor],
) -> bool {
    match capability {
        ResolvedCapability::WorkspaceFileRead => true,
        ResolvedCapability::BrowserRead => {
            !browser_navigation(resources)
                || mode != PermissionMode::ReadOnly
                || browser_site(resources).is_some_and(|(_, rule)| rule.is_some())
        }
        ResolvedCapability::BrowserInteraction => mode != PermissionMode::ReadOnly,
        _ => false,
    }
}
