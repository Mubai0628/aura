use super::*;

/// Encode the old on-disk shape, without the new defaulted lifetime marker.
pub(super) fn legacy(f: &mut Fixture, grants: Vec<AuthorizationGrant>) -> Vec<AuthorizationGrant> {
    f.store
        .database
        .unit_of_work(|tx| {
            for grant in &grants {
                let mut value = serde_json::to_value(grant)?;
                value["scope"]["consent"]
                    .as_object_mut()
                    .expect("native reference")
                    .remove("lifetime");
                value["scope"]
                    .as_object_mut()
                    .expect("historical scope")
                    .remove("capability_limit");
                tx.execute(
                    "UPDATE authorization_grants SET payload=?2 WHERE id=?1",
                    rusqlite::params![grant.grant_id.to_string(), serde_json::to_string(&value)?],
                )?;
                let consent =
                    security::native_session_consent(tx, grant.grant_id)?.expect("native row");
                let mut value = serde_json::to_value(consent)?;
                value.as_object_mut().expect("native record").remove("lifetime");
                tx.execute(
                    "UPDATE native_session_consents SET payload=?2 WHERE grant_id=?1",
                    rusqlite::params![grant.grant_id.to_string(), serde_json::to_string(&value)?],
                )?;
            }
            Ok::<_, aura_persistence_core::PersistenceCoreError>(())
        })
        .expect("write historical shape only to the isolated fixture database");
    grants
        .into_iter()
        .map(|grant| {
            security::grant(f.store.database.connection(), grant.grant_id)
                .expect("historical grant")
                .expect("present")
        })
        .collect()
}
