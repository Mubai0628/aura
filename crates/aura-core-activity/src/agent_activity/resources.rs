//! Copy verified executor facts into the independent information contract. No file IO.
use aura_activity_domain::{ActivityResource, ActivityResourceRelation, ActivityResourceTarget};
use aura_workspace_domain::{FileResource, ResourceAccess, ResourceRequest, SlotCaseRule};

pub fn project(requests: &[ResourceRequest]) -> Vec<ActivityResource> {
    requests
        .iter()
        // Retain one overflow sentinel: the index rejects the row and reports Partial.
        // Silently clipping a write set would falsely claim complete observation.
        .take(aura_activity_domain::MAX_ACTIVITY_RESOURCES + 1)
        .map(|request| {
            let (target, owner) = match &request.resource {
                FileResource::Object { identity, location, owner } => (
                    ActivityResourceTarget::VerifiedObject {
                        identity: identity.clone(),
                        ancestors: location.chain().cloned().collect(),
                        subtree: false,
                    },
                    owner,
                ),
                FileResource::Subtree { directory_identity, location, owner } => (
                    ActivityResourceTarget::VerifiedObject {
                        identity: directory_identity.clone(),
                        ancestors: location.chain().cloned().collect(),
                        subtree: true,
                    },
                    owner,
                ),
                FileResource::Slot { parent, leaf_name, owner } => (
                    match parent.comparison_key(leaf_name) {
                        Some(key) => ActivityResourceTarget::VerifiedEntry {
                            parent: parent.identity.clone(),
                            pending: vec![leaf_name.clone()],
                            case_insensitive: parent.case_rule
                                == SlotCaseRule::InsensitivePreserving,
                            comparison_keys: Some(vec![key]),
                            ancestors: parent.location.chain().cloned().collect(),
                        },
                        None => ActivityResourceTarget::Unproven {
                            hint: "entry comparison unavailable".into(),
                        },
                    },
                    owner,
                ),
                FileResource::ProspectiveEntry { parent, pending, owner } => (
                    match pending
                        .iter()
                        .map(|name| parent.comparison_key(name))
                        .collect::<Option<Vec<_>>>()
                    {
                        Some(keys) => ActivityResourceTarget::VerifiedEntry {
                            parent: parent.identity.clone(),
                            pending: pending.clone(),
                            case_insensitive: parent.case_rule
                                == SlotCaseRule::InsensitivePreserving,
                            comparison_keys: Some(keys),
                            ancestors: parent.location.chain().cloned().collect(),
                        },
                        None => ActivityResourceTarget::Unproven {
                            hint: "entry comparison unavailable".into(),
                        },
                    },
                    owner,
                ),
            };
            let path = owner.path.as_str();
            ActivityResource {
                target,
                relation: if request.access == ResourceAccess::Read {
                    ActivityResourceRelation::Read
                } else {
                    ActivityResourceRelation::Write
                },
                display_name: super::truncate_display(path),
                display_name_truncated: path.len()
                    > aura_activity_domain::MAX_ACTIVITY_RESOURCE_DISPLAY_BYTES,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_over_limit_write_set_is_reported_partial_instead_of_silently_clipped() {
        use aura_domain_common::*;
        use aura_workspace_domain::*;
        let identity = FileIdentity { volume_serial_number: "1".into(), file_id: "aa".repeat(16) };
        let owner = ResourceOwner {
            workspace_id: WorkspaceId::new(),
            session_id: SessionId::new(),
            path: "target".parse().expect("verified execution resource fixture"),
        };
        let requests = (0..aura_activity_domain::MAX_ACTIVITY_RESOURCES + 8)
            .map(|_| ResourceRequest {
                resource: FileResource::Object {
                    identity: identity.clone(),
                    location: ResourceLocation::at_anchor(identity.clone()),
                    owner: owner.clone(),
                },
                access: ResourceAccess::Write,
            })
            .collect::<Vec<_>>();
        let offered = project(&requests);
        assert_eq!(offered.len(), aura_activity_domain::MAX_ACTIVITY_RESOURCES + 1);
        let index = aura_activity_runtime::ActivityIndex::default();
        let mut record = crate::agent_activity::tests_support::synthetic_record();
        let view = aura_activity_runtime::ActivityVisibility::for_session(
            record.runtime_instance_id,
            record.owner.session_id,
            record.owner.workspace_id,
        );
        record.resources = offered;
        assert_eq!(index.publish(record), aura_activity_domain::ActivityPublishOutcome::Rejected);
        let picture = index.snapshot(&view);
        assert_eq!(
            picture.header.coverage.state,
            aura_activity_domain::ActivityCoverageState::Partial
        );
        assert_eq!(picture.header.coverage.rejected, 1);
        assert!(picture.rows.is_empty());
    }
}
