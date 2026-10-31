//! Global defaults are read at session creation; changing them never rewrites a session.
use super::*;
use aura_config::SettingsFacadeError;
use aura_core_base::config_degradation;
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
                config_degradation::config_fault_code(error),
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
