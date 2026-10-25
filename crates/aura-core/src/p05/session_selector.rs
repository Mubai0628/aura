//! A new Session does not require a later configuration edit to become readable.
use super::P05Services;
use aura_domain_common::SessionId;
use aura_projection_contract::{ProjectionChange, ProjectionKind, ProjectionScope};

impl P05Services {
    pub(crate) fn session_selector(
        &self,
        session_id: SessionId,
    ) -> anyhow::Result<ProjectionChange> {
        let state = self.persistence.session_model_binding_state(session_id)?;
        if let Some(change) = self.persistence.latest_model_projection(
            ProjectionKind::SessionModelSelector,
            ProjectionScope::Session { session_id },
        )? && matches!(&change, ProjectionChange::SessionModelSelector { payload, .. }
            if payload.binding_revision == state.revision &&
                payload.selected_model_profile_id == state.binding.as_ref().map(|binding| binding.model_profile_id))
        {
            return Ok(change);
        }
        let proposed = self.projections.session_model_selector(
            &self.model_config.snapshot()?,
            session_id,
            state.binding,
        )?;
        Ok(self.persistence.ensure_session_model_selector(proposed)?)
    }
}
