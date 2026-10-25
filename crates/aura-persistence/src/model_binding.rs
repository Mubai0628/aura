use std::{collections::BTreeSet, str::FromStr};

use aura_domain_common::{
    CorrelationId, ExpectedRevision, ModelProfileId, Revision, SessionId, UtcTimestamp, WorkspaceId,
};
use aura_model_domain::{
    SessionModelBinding, SessionModelBindingChangedPayload, SessionModelSelectorProjection,
};
use aura_projection_contract::{ProjectionChange, ProjectionKey, ProjectionKind, ProjectionScope};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::{
    EventActor, ModelProjectionDraft, PersistenceCoordinator, PersistenceError,
    PersistentEventDraft, coordinator::insert_event, model_projection::insert_model_projections,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionModelBindingState {
    pub revision: Revision,
    pub binding: Option<SessionModelBinding>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionModelBindingCommit {
    pub changed: bool,
    pub revision: Revision,
    pub binding: Option<SessionModelBinding>,
    pub event_row_id: Option<i64>,
    pub projection_changes: Vec<ProjectionChange>,
    pub projection_outbox_ids: Vec<i64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedSessionModelBindingClear {
    pub session_id: SessionId,
    pub expected_binding_revision: Revision,
    pub expected_model_profile_id: ModelProfileId,
}

impl PersistenceCoordinator {
    /// Initializes or repairs only the selector cache. The binding remains the
    /// sole authority and is checked inside the same writer transaction.
    pub fn ensure_session_model_selector(
        &self,
        proposed: SessionModelSelectorProjection,
    ) -> Result<ProjectionChange, PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        let change = self.dispatch(move |connection| {
            let tx = connection.transaction()?;
            session_workspace_id(&tx, proposed.session_id)?;
            let state = load_session_model_binding_state(&tx, proposed.session_id)?;
            let selected = state.binding.as_ref().map(|value| value.model_profile_id);
            let key = ProjectionKey { runtime_instance_id,
                projection_kind: ProjectionKind::SessionModelSelector,
                projection_scope: ProjectionScope::Session { session_id: proposed.session_id } };
            let previous: Option<String> = tx.query_row(
                "SELECT payload_json FROM projection_outbox WHERE projection_kind='session_model_selector'
                 AND projection_key=?1 ORDER BY revision DESC,outbox_id DESC LIMIT 1",
                [serde_json::to_string(&key)?], |row| row.get(0)).optional()?;
            if let Some(previous) = previous {
                let previous: ProjectionChange = serde_json::from_str(&previous)?;
                if matches!(&previous, ProjectionChange::SessionModelSelector { payload, .. }
                    if payload.binding_revision == state.revision && payload.selected_model_profile_id == selected) {
                    return Ok(previous);
                }
            }
            if proposed.binding_revision != state.revision || proposed.selected_model_profile_id != selected {
                return Err(PersistenceError::SessionModelBindingRevisionConflict);
            }
            let (mut changes, _) = insert_model_projections(&tx, runtime_instance_id,
                vec![ModelProjectionDraft::SessionModelSelector(proposed)])?;
            tx.commit()?;
            Ok(changes.remove(0))
        })?;
        self.publish_model_projection_changes(std::slice::from_ref(&change))?;
        Ok(change)
    }

    pub fn commit_session_model_binding(
        &self,
        request: CommitSessionModelBinding,
    ) -> Result<SessionModelBindingCommit, PersistenceError> {
        let CommitSessionModelBinding {
            session_id,
            expected_revision,
            model_profile_id,
            readable_ref_at_binding,
            bound_at,
            correlation_id,
            atomic_group_id,
            mut selector_projection,
        } = request;
        match (model_profile_id, readable_ref_at_binding.as_deref()) {
            (Some(_), Some(readable_ref)) if !readable_ref.trim().is_empty() => {}
            (None, None) => {}
            _ => {
                return Err(PersistenceError::InvalidRequest(
                    "session model binding target and readable reference must be present together"
                        .to_owned(),
                ));
            }
        }
        if selector_projection.session_id != session_id {
            return Err(PersistenceError::InvalidRequest(
                "session model selector projection belongs to another Session".to_owned(),
            ));
        }

        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let workspace_id = session_workspace_id(&transaction, session_id)?;
            let current = load_session_model_binding_state(&transaction, session_id)?;
            if current.revision.value() != expected_revision.value() {
                return Err(PersistenceError::SessionModelBindingRevisionConflict);
            }
            if current.binding.as_ref().map(|binding| binding.model_profile_id) == model_profile_id
            {
                return Ok(SessionModelBindingCommit {
                    changed: false,
                    revision: current.revision,
                    binding: current.binding,
                    event_row_id: None,
                    projection_changes: Vec::new(),
                    projection_outbox_ids: Vec::new(),
                });
            }

            let next_revision = current.revision.value().checked_add(1).ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "session model binding revision is exhausted".to_owned(),
                )
            })?;
            let next_binding = model_profile_id.map(|model_profile_id| SessionModelBinding {
                session_id,
                model_profile_id,
                binding_revision: Revision::new(next_revision),
                bound_at,
                readable_ref_at_binding: readable_ref_at_binding
                    .clone()
                    .expect("validated binding reference is present"),
            });
            transaction.execute(
                "INSERT INTO session_model_bindings(
                    session_id, model_profile_id, binding_revision, bound_at,
                    readable_ref_at_binding
                 ) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(session_id) DO UPDATE SET
                    model_profile_id=excluded.model_profile_id,
                    binding_revision=excluded.binding_revision,
                    bound_at=excluded.bound_at,
                    readable_ref_at_binding=excluded.readable_ref_at_binding",
                params![
                    session_id.to_string(),
                    model_profile_id.map(|id| id.to_string()),
                    to_sql_revision(next_revision)?,
                    next_binding.as_ref().map(|binding| binding.bound_at.to_string()),
                    next_binding.as_ref().map(|binding| binding.readable_ref_at_binding.as_str()),
                ],
            )?;

            let payload = SessionModelBindingChangedPayload {
                session_id,
                previous_binding: current.binding,
                binding: next_binding.clone(),
                binding_revision: Revision::new(next_revision),
            };
            let event = PersistentEventDraft {
                event_id: aura_domain_common::EventId::new(),
                workspace_id: Some(workspace_id),
                session_id: Some(session_id),
                turn_task_id: None,
                agent_run_id: None,
                event_type: "SessionModelBindingChanged".to_owned(),
                event_version: 1,
                actor: EventActor::User,
                correlation_id,
                causation_id: None,
                atomic_group_id,
                payload_json: Some(serde_json::to_value(payload)?),
                payload_artifact_ref: None,
                searchable_text: None,
                created_at: bound_at,
            };
            let event_row_id = insert_event(&transaction, &event)?;
            selector_projection.binding_revision = Revision::new(next_revision);
            selector_projection.selected_model_profile_id = model_profile_id;
            let (projection_changes, projection_outbox_ids) = insert_model_projections(
                &transaction,
                runtime_instance_id,
                vec![ModelProjectionDraft::SessionModelSelector(selector_projection)],
            )?;
            transaction.commit()?;
            Ok(SessionModelBindingCommit {
                changed: true,
                revision: Revision::new(next_revision),
                binding: next_binding,
                event_row_id: Some(event_row_id),
                projection_changes,
                projection_outbox_ids,
            })
        })
    }

    pub fn session_model_binding_state(
        &self,
        session_id: SessionId,
    ) -> Result<SessionModelBindingState, PersistenceError> {
        self.with_reader(|connection| load_session_model_binding_state(connection, session_id))
    }

    pub fn session_model_binding(
        &self,
        session_id: SessionId,
    ) -> Result<Option<SessionModelBinding>, PersistenceError> {
        Ok(self.session_model_binding_state(session_id)?.binding)
    }

    pub fn session_model_bindings_for_profiles(
        &self,
        model_profile_ids: &BTreeSet<ModelProfileId>,
    ) -> Result<Vec<SessionModelBinding>, PersistenceError> {
        if model_profile_ids.is_empty() {
            return Ok(Vec::new());
        }
        let model_profile_ids = model_profile_ids.clone();
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT session_id, model_profile_id, binding_revision, bound_at,
                        readable_ref_at_binding
                 FROM session_model_bindings
                 WHERE model_profile_id IS NOT NULL
                 ORDER BY session_id",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut bindings = Vec::new();
            for (
                session_id,
                model_profile_id,
                binding_revision,
                bound_at,
                readable_ref_at_binding,
            ) in rows
            {
                let model_profile_id = ModelProfileId::from_str(&model_profile_id)?;
                if !model_profile_ids.contains(&model_profile_id) {
                    continue;
                }
                bindings.push(SessionModelBinding {
                    session_id: SessionId::from_str(&session_id)?,
                    model_profile_id,
                    binding_revision: Revision::new(u64::try_from(binding_revision).map_err(
                        |_| {
                            PersistenceError::InvalidRequest(
                                "persisted session model binding revision is invalid".to_owned(),
                            )
                        },
                    )?),
                    bound_at: bound_at.parse().map_err(|_| {
                        PersistenceError::InvalidRequest(
                            "persisted session model binding timestamp is invalid".to_owned(),
                        )
                    })?,
                    readable_ref_at_binding,
                });
            }
            Ok(bindings)
        })
    }
}

#[derive(Clone, Debug)]
pub struct CommitSessionModelBinding {
    pub session_id: SessionId,
    pub expected_revision: ExpectedRevision,
    pub model_profile_id: Option<ModelProfileId>,
    pub readable_ref_at_binding: Option<String>,
    pub bound_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
    pub atomic_group_id: Option<String>,
    pub selector_projection: SessionModelSelectorProjection,
}

fn session_workspace_id(
    transaction: &Transaction<'_>,
    session_id: SessionId,
) -> Result<WorkspaceId, PersistenceError> {
    let workspace_id = transaction
        .query_row(
            "SELECT workspace_id FROM sessions WHERE session_id=?1",
            [session_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(PersistenceError::InvalidRequest(
            "session model binding target Session does not exist".to_owned(),
        ))?;
    Ok(WorkspaceId::from_str(&workspace_id)?)
}

fn load_session_model_binding_state(
    connection: &rusqlite::Connection,
    session_id: SessionId,
) -> Result<SessionModelBindingState, PersistenceError> {
    let row = connection
        .query_row(
            "SELECT model_profile_id, binding_revision, bound_at, readable_ref_at_binding
             FROM session_model_bindings WHERE session_id=?1",
            [session_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((model_profile_id, revision, bound_at, readable_ref_at_binding)) = row else {
        return Ok(SessionModelBindingState { revision: Revision::new(0), binding: None });
    };
    let revision = u64::try_from(revision).map_err(|_| {
        PersistenceError::InvalidRequest(
            "persisted session model binding revision is invalid".to_owned(),
        )
    })?;
    let binding = match (model_profile_id, bound_at, readable_ref_at_binding) {
        (None, None, None) => None,
        (Some(model_profile_id), Some(bound_at), Some(readable_ref_at_binding)) => {
            Some(SessionModelBinding {
                session_id,
                model_profile_id: ModelProfileId::from_str(&model_profile_id)?,
                binding_revision: Revision::new(revision),
                bound_at: bound_at.parse().map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "persisted session model binding timestamp is invalid".to_owned(),
                    )
                })?,
                readable_ref_at_binding,
            })
        }
        _ => {
            return Err(PersistenceError::InvalidRequest(
                "persisted session model binding row is internally inconsistent".to_owned(),
            ));
        }
    };
    Ok(SessionModelBindingState { revision: Revision::new(revision), binding })
}

pub(crate) fn clear_prepared_session_model_bindings(
    transaction: &Transaction<'_>,
    clears: &[PreparedSessionModelBindingClear],
) -> Result<(), PersistenceError> {
    for clear in clears {
        let current = load_session_model_binding_state(transaction, clear.session_id)?;
        if current.revision != clear.expected_binding_revision
            || current.binding.as_ref().map(|binding| binding.model_profile_id)
                != Some(clear.expected_model_profile_id)
        {
            return Err(PersistenceError::SessionModelBindingRevisionConflict);
        }
        let next_revision = current.revision.value().checked_add(1).ok_or_else(|| {
            PersistenceError::InvalidRequest(
                "session model binding revision is exhausted".to_owned(),
            )
        })?;
        let changed = transaction.execute(
            "UPDATE session_model_bindings
             SET model_profile_id=NULL, binding_revision=?1, bound_at=NULL,
                 readable_ref_at_binding=NULL
             WHERE session_id=?2 AND binding_revision=?3 AND model_profile_id=?4",
            params![
                to_sql_revision(next_revision)?,
                clear.session_id.to_string(),
                to_sql_revision(clear.expected_binding_revision.value())?,
                clear.expected_model_profile_id.to_string(),
            ],
        )?;
        if changed != 1 {
            return Err(PersistenceError::SessionModelBindingRevisionConflict);
        }
    }
    Ok(())
}

fn to_sql_revision(revision: u64) -> Result<i64, PersistenceError> {
    i64::try_from(revision).map_err(|_| {
        PersistenceError::InvalidRequest(
            "session model binding revision exceeds SQLite range".to_owned(),
        )
    })
}
