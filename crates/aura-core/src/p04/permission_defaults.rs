//! Global defaults are read at session creation; changing them never rewrites a session.
use super::*;
use aura_config::SettingsFacadeError;
use aura_permission_domain::{PermissionDefaults, SetPermissionDefaultsInput};

/// Reads the global permission defaults from configuration, or reports why not.
///
/// P04's startup used to `?` each configuration read directly, so a
/// damaged `settings/security.toml` or `aura.toml` aborted the whole Core
/// runtime. Reading the two domains a permission default genuinely needs is now
/// one operation with one real error, so the caller can attribute the fault to
/// P04's security authority and refuse **only** that.
///
/// No default is fabricated here. A default permission mode invented from a read
/// failure would be an invented authority, and if it happened to be looser than
/// the stored one it would silently widen permissions.
pub(crate) fn permission_defaults_from_settings(
    settings: &SettingsFacade,
) -> Result<PermissionDefaults, PermissionDefaultsFault> {
    let security =
        settings.snapshot(ConfigModule::Security).map_err(PermissionDefaultsFault::Config)?;
    // The security document must really validate; reading it is not enough.
    SecurityConfig::from_document(&security.document).map_err(|source| {
        PermissionDefaultsFault::Invalid {
            domain: ConfigModule::Security,
            message: source.to_string(),
        }
    })?;
    let aura = settings.snapshot(ConfigModule::Aura).map_err(PermissionDefaultsFault::Config)?;
    let mode = parse_default_permission_mode(&aura.document).map_err(|source| {
        PermissionDefaultsFault::Invalid { domain: ConfigModule::Aura, message: source.to_string() }
    })?;
    Ok(PermissionDefaults { revision: aura.document.revision, mode })
}

/// Why the global permission defaults could not be read.
///
/// A typed reason rather than a string so the caller can attribute the fault to
/// a domain and a stable code. The raw message is kept for the configuration
/// layer's own output; it does not become the projection's text.
#[derive(Debug)]
pub(crate) enum PermissionDefaultsFault {
    /// A configuration domain could not be read.
    Config(SettingsFacadeError),
    /// A domain was read but its document did not validate.
    Invalid { domain: ConfigModule, message: String },
}

impl PermissionDefaultsFault {
    /// The stable fault code and the domain that caused it.
    ///
    /// The raw input does not enter ordinary output: what a log or a projection
    /// carries is the bounded code and the domain key.
    #[must_use]
    pub(crate) fn stable_code_and_domain(&self) -> (&'static str, &'static str) {
        match self {
            Self::Config(error) => (
                crate::config_degradation::config_fault_code(error),
                error.module().map_or("unknown", ConfigModule::key),
            ),
            Self::Invalid { domain, .. } => ("config.domain.validation", domain.key()),
        }
    }
}

impl std::fmt::Display for PermissionDefaultsFault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(error) => write!(formatter, "{error}"),
            Self::Invalid { domain, message } => {
                write!(formatter, "configuration for {} is invalid: {message}", domain.key())
            }
        }
    }
}

impl std::error::Error for PermissionDefaultsFault {}

impl P04Services {
    pub(crate) fn permission_defaults(&self) -> Result<PermissionDefaults> {
        let snapshot = self.settings.snapshot(ConfigModule::Aura)?;
        Ok(PermissionDefaults {
            revision: snapshot.document.revision,
            mode: parse_default_permission_mode(&snapshot.document)?,
        })
    }

    pub(super) fn set_permission_defaults(
        &self,
        input: SetPermissionDefaultsInput,
        correlation_id: CorrelationId,
    ) -> Result<PermissionDefaults, IpcErrorEnvelope> {
        let mode = serde_json::to_value(input.mode)
            .map_err(|error| internal_error(error, correlation_id))?;
        let snapshot = self
            .settings
            .update(ConfigModule::Aura, input.expected_revision, |values| {
                values.insert("default_permission_mode".to_owned(), mode);
            })
            .map_err(|error| match error {
                SettingsFacadeError::RevisionConflict { .. } => envelope(
                    "config.revision_conflict",
                    ErrorCategory::Conflict,
                    "默认设置已更新，请重新读取后再保存。",
                    false,
                    correlation_id,
                ),
                other => internal_error(other, correlation_id),
            })?;
        Ok(PermissionDefaults { revision: snapshot.document.revision, mode: input.mode })
    }
}

#[cfg(test)]
mod tests {
    use crate::r03_tests::{Fixture, request, success};
    use serde_json::json;

    #[tokio::test]
    async fn defaults_only_affect_new_sessions_and_reject_stale_writes() {
        let fixture = Fixture::new();
        let state = fixture.start().await;
        let root = fixture.root.path().join("permission-defaults-workspace");
        std::fs::create_dir(&root).expect("workspace");
        let workspace = success(
            &state,
            "workspace.create",
            json!({"selected_root":root,"display_name":"defaults"}),
        )
        .await;
        let workspace_id = &workspace["workspace"]["workspace_id"];
        let before = success(&state, "permission.get_defaults", json!({})).await;
        let first = success(
            &state,
            "session.create",
            json!({"workspace_id":workspace_id,"title":"existing"}),
        )
        .await;
        let first_permission = success(
            &state,
            "permission.get_session_state",
            json!({"session_id":first["session_id"]}),
        )
        .await;
        let next_mode =
            if before["mode"] == "read_only" { "workspace_read_write" } else { "read_only" };
        let saved = success(
            &state,
            "permission.set_defaults",
            json!({"expected_revision":before["revision"],"mode":next_mode}),
        )
        .await;
        assert_eq!(saved["mode"], next_mode);
        let second =
            success(&state, "session.create", json!({"workspace_id":workspace_id,"title":"new"}))
                .await;
        assert_eq!(
            success(
                &state,
                "permission.get_session_state",
                json!({"session_id":second["session_id"]})
            )
            .await["mode"],
            next_mode
        );
        assert_eq!(
            success(
                &state,
                "permission.get_session_state",
                json!({"session_id":first["session_id"]})
            )
            .await,
            first_permission
        );
        assert!(
            request(
                &state,
                "permission.set_defaults",
                json!({"expected_revision":before["revision"],"mode":"full_access"})
            )
            .await
            .get("error")
            .is_some()
        );
        assert_eq!(success(&state, "permission.get_defaults", json!({})).await, saved);
        drop(state);
        let reopened = fixture.start().await;
        assert_eq!(success(&reopened, "permission.get_defaults", json!({})).await, saved);
        assert_eq!(
            success(
                &reopened,
                "permission.get_session_state",
                json!({"session_id":first["session_id"]})
            )
            .await,
            first_permission
        );
    }
}
