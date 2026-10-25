//! One configured destination, resolved by Core before asking for permission.
use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitRemoteTargetV1 {
    /// Hash of the retained common-dir's volume and file identity, not its spelling.
    pub repository: aura_domain_common::jcs::CanonicalSha256HashV1,
    pub remote_url: String,
    /// Binds non-display URL components too; raw query data is never persisted.
    pub endpoint_fingerprint: aura_domain_common::jcs::CanonicalSha256HashV1,
    pub remote_branch: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitRemoteFactsV1 {
    pub target: GitRemoteTargetV1,
    pub remote: String,
    pub local_branch: Option<String>,
    pub local_head: Option<String>,
    pub force_with_lease: Option<String>,
    /// Based only on the last fetched tracking ref. None means no such ref exists.
    pub outgoing_count: Option<u32>,
    pub outgoing_subjects: Vec<String>,
}

pub fn git_remote_facts(resources: &[ResourceDescriptor]) -> Option<&GitRemoteFactsV1> {
    let mut found = resources.iter().filter_map(|resource| match resource {
        ResourceDescriptor::GitRemoteDestination { facts } => Some(facts),
        _ => None,
    });
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

pub fn native_git_remote(
    source: &SecuritySourceRef,
    action: &str,
    caps: &[ResolvedCapability],
) -> bool {
    matches!(action, "git.sync" | "git.push") && built_in_native_git(source, action, caps)
}

pub fn git_push_consent_target<'a>(
    source: &SecuritySourceRef,
    action: &str,
    caps: &[ResolvedCapability],
    resources: &'a [ResourceDescriptor],
) -> Option<&'a GitRemoteTargetV1> {
    if action != "git.push"
        || caps != [ResolvedCapability::GitRemoteWrite]
        || !built_in_native_git(source, action, caps)
    {
        return None;
    }
    let facts = git_remote_facts(resources)?;
    (facts.force_with_lease.is_none() && !facts.target.remote_branch.is_empty())
        .then_some(&facts.target)
}
