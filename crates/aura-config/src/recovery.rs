//! The user's recovery entry for a damaged non-secret configuration.
//!
//! # What this module is for
//!
//! R1–R4 make a damaged configuration domain *survivable*: unrelated domains keep
//! working, dependents are refused honestly, and the fault is reported with a
//! stable code. Survivable is not the same as **recoverable**. Without an entry
//! the user can reach, "the domain is degraded and other domains continue" is just
//! a log line: the domain stays broken forever.
//!
//! This module provides the three options the user confirmed:
//!
//! 1. **Retry** — try the current file again, now.
//! 2. **Restore the previous valid configuration** — re-apply a version that
//!    really passed validation, from the domain's own recorded history.
//! 3. **Reset to a validated default** — write the domain's default *through the
//!    domain's own validator*, so the result is a legitimate configuration rather
//!    than an invented one.
//!
//! # The rules this implementation is held to
//!
//! * **Preview first**: [`RecoveryPlan`] is computed and returned *without writing
//!   anything*. The user sees the scope, the references that would need rebinding
//!   and the actual loss, and only then triggers the action.
//! * **A new revision**: recovery writes revision `current + 1` and never rewinds
//!   the file's revision, so the CAS that protects every other writer still holds.
//! * **Real history only**: the previous version comes from the domain's own
//!   `version-*` records, which exist only because a document really passed
//!   validation. It is **re-validated under the current validator and the current
//!   permission** before it may be applied; a version that no longer passes is
//!   reported as unusable rather than applied.
//! * **Honest refusal**: when no legitimate previous version exists, the option is
//!   *disabled in the plan*, not silently substituted by something else.
//! * **No collateral damage**: recovery never deletes credentials, sessions or
//!   workspaces, and never clears the unresolved records that R2 protects.
//! * **Bounded**: a failed recovery does not remove stop/diagnostic ability, does
//!   not retry forever, and does not hide the failure.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use aura_domain_common::UtcTimestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AuraPaths, ConfigModule, ConfigSource, DomainFaultKind, SettingsFacadeError,
    document::{
        ModuleConfigDocument, ModuleConfigValidator, encode_document, parse_document,
        validate_base_document,
    },
    validate_builtin_module_document,
};

/// Which recovery action the user is asking for.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigRecoveryAction {
    /// Try the domain's current file again, changing nothing on disk.
    Retry,
    /// Re-apply the most recent recorded version that still passes current
    /// validation.
    RestorePreviousValid,
    /// Write the domain's validated default.
    ResetToDefault,
}

impl ConfigRecoveryAction {
    /// A bounded, renderer-safe key for this action.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::RestorePreviousValid => "restore_previous_valid",
            Self::ResetToDefault => "reset_to_default",
        }
    }
}

/// Whether one recovery option is available, and exactly why not when it is not.
///
/// An unavailable option is **stated**, never silently omitted and never replaced
/// by a different action. That is what "honestly disable the option" means.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecoveryOption {
    pub action: ConfigRecoveryAction,
    pub available: bool,
    /// A stable code when `available` is false or when the option carries a
    /// caveat. No path, no document body, nothing the user supplied.
    pub reason_code: Option<String>,
    /// A bounded, path-free explanation for the user.
    pub detail: Option<String>,
}

impl RecoveryOption {
    fn available(action: ConfigRecoveryAction) -> Self {
        Self { action, available: true, reason_code: None, detail: None }
    }

    fn unavailable(
        action: ConfigRecoveryAction,
        reason_code: &str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            action,
            available: false,
            reason_code: Some(reason_code.to_owned()),
            detail: Some(detail.into()),
        }
    }

    /// An available option, with a bounded explanation of what it would do.
    #[must_use]
    pub fn available_with_detail(action: ConfigRecoveryAction, detail: impl Into<String>) -> Self {
        Self { action, available: true, reason_code: None, detail: Some(detail.into()) }
    }

    /// An available option, constructed from outside this module.
    #[must_use]
    pub fn available_for_plan(action: ConfigRecoveryAction) -> Self {
        Self::available(action)
    }

    /// An **unavailable** option, with the stable reason it cannot be offered.
    ///
    /// The option stays in the list: an action the user might expect to exist is
    /// shown as unavailable with its reason, so it is neither silently missing nor
    /// quietly replaced by a different action.
    #[must_use]
    pub fn unavailable_for_plan(
        action: ConfigRecoveryAction,
        reason_code: &str,
        detail: impl Into<String>,
    ) -> Self {
        Self::unavailable(action, reason_code, detail)
    }
}

/// One recorded version the domain could be restored to.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecoverableVersion {
    pub revision: u64,
    /// Whether this version still passes the **current** validator and permission.
    pub passes_current_validation: bool,
    /// A stable code when it does not.
    pub rejection_code: Option<String>,
}

/// What one domain's recovery would do, before anything is written.
///
/// This is the **preview**. It names the affected domain, the options with their
/// real availability, the versions that could be restored, the references that
/// would need rebinding, and the actual loss.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DomainRecoveryPlan {
    pub module: ConfigModule,
    /// The domain's current state, as the health registry reports it.
    pub degraded: bool,
    /// The stable classification of the fault that made it degraded, if any.
    pub fault_code: Option<String>,
    /// The revision currently served, if the domain serves anything.
    pub current_revision: Option<u64>,
    pub options: Vec<RecoveryOption>,
    /// The recorded versions, newest first, with their current-validity verdict.
    pub recoverable_versions: Vec<RecoverableVersion>,
    /// The revision the recovery result would carry: always `current + 1`.
    pub next_revision: u64,
    /// Settings keys that reference something outside the domain and would need
    /// rebinding after a restore or reset. Names only; never their values.
    pub references_needing_rebinding: Vec<String>,
    /// What the user actually loses by restoring the previous version. Stated so
    /// the choice is informed rather than a guess.
    pub loss: Vec<String>,
    /// Whether the runtime could establish this domain's recovery scope at all.
    pub scope_known: bool,
    /// The identity of the state this preview was computed from.
    ///
    /// A confirmation must be bound to what
    /// the user was actually shown. Measured defect: a preview of a rev1 reset was
    /// still applied after a legitimate save to dark/rev2, so the stale
    /// confirmation overwrote a newer, valid configuration — `current_revision` was
    /// not part of any check at commit time.
    ///
    /// The caller echoes this back and the write refuses when the domain has moved
    /// since. It is derived from the real facts the preview was built from, so a
    /// caller cannot forge a preview for a state it was never shown.
    pub preview_id: String,
}

impl DomainRecoveryPlan {
    /// The option the user asked about, if the plan offers it.
    #[must_use]
    pub fn option(&self, action: ConfigRecoveryAction) -> Option<&RecoveryOption> {
        self.options.iter().find(|option| option.action == action)
    }

    /// Whether an action may be applied, per this preview.
    #[must_use]
    pub fn allows(&self, action: ConfigRecoveryAction) -> bool {
        self.option(action).is_some_and(|option| option.available)
    }
}

/// The plan for a whole cross-domain recovery.
///
/// A cross-domain restore must reach **one provable consistent version**,
/// so the bundle plan is computed as a unit. It never staples each domain's own
/// last-known-good together and calls the result consistent: either a common
/// revision is provable, or the cross-domain restore is honestly disabled and the
/// user is offered a scoped default reset instead.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BundleRecoveryPlan {
    /// Core supplies this opaque binding; clients echo it without reimplementing hashing.
    pub preview_id: String,
    pub modules: Vec<ConfigModule>,
    pub options: Vec<RecoveryOption>,
    /// The common revision every affected domain can reach, when one exists.
    pub consistent_revision: Option<u64>,
    /// Per-domain plans, so the user sees the whole scope before triggering.
    pub domains: Vec<DomainRecoveryPlan>,
    /// A bounded explanation when a consistent version cannot be proven.
    pub inconsistency_code: Option<String>,
    /// The exact set the próven consistent version restores, when one exists.
    ///
    /// This is the preview's own binding. `apply` consumes
    /// **this** list rather than re-selecting each domain's own latest history, so
    /// the write cannot silently differ from what the user was shown. Each entry is
    /// `(module, revision, content digest of the target version)`.
    ///
    /// It is empty exactly when no joint record proved a consistent set, which is
    /// also when `consistent_revision` is `None`.
    pub consistent_set: Vec<(ConfigModule, u64, String)>,
}

impl BundleRecoveryPlan {
    #[must_use]
    pub fn allows(&self, action: ConfigRecoveryAction) -> bool {
        self.options.iter().any(|option| option.action == action && option.available)
    }

    /// The bundle-level option the user asked about, if the plan offers it.
    #[must_use]
    pub fn option(&self, action: ConfigRecoveryAction) -> Option<&RecoveryOption> {
        self.options.iter().find(|option| option.action == action)
    }
}

/// What one recovery action really did.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecoveryOutcome {
    pub module: ConfigModule,
    pub action: ConfigRecoveryAction,
    pub applied: bool,
    /// The new revision the domain now serves, when the action applied.
    pub revision: Option<u64>,
    /// The source the recovered snapshot came from.
    pub source: Option<ConfigSource>,
    /// A stable code when the action did not apply.
    pub reason_code: Option<String>,
}

/// A recovery attempt that could not be planned or applied, with its real reason.
#[derive(Debug, thiserror::Error)]
pub enum ConfigRecoveryError {
    #[error("the configuration recovery plan could not be built for {module:?}: {source}")]
    Plan { module: ConfigModule, source: Box<SettingsFacadeError> },
    #[error("configuration recovery for {module:?} is unavailable: {reason_code}")]
    Unavailable { module: ConfigModule, reason_code: &'static str },
    #[error("configuration recovery for {module:?} failed: {source}")]
    Apply { module: ConfigModule, source: Box<SettingsFacadeError> },
}

/// Stable codes this module reports.
pub mod codes {
    /// No recorded version passes the current validator, so the restore option is
    /// honestly disabled.
    pub const NO_VALID_PREVIOUS_VERSION: &str = "config.recovery.no_valid_previous_version";
    /// The domain has no recorded history at all.
    pub const NO_HISTORY: &str = "config.recovery.no_history";
    /// The domain serves nothing, so there is no current revision to advance.
    pub const NO_CURRENT_SNAPSHOT: &str = "config.recovery.no_current_snapshot";
    /// A cross-domain consistent version could not be proven.
    pub const NO_CONSISTENT_BUNDLE_VERSION: &str = "config.recovery.no_consistent_bundle_version";
    /// The action is not offered by the plan.
    pub const ACTION_NOT_OFFERED: &str = "config.recovery.action_not_offered";
    /// The action was offered but is not currently applicable.
    pub const ACTION_UNAVAILABLE: &str = "config.recovery.action_unavailable";
    /// The domain's recovery scope could not be established.
    pub const SCOPE_UNKNOWN: &str = "config.recovery.scope_unknown";
    /// The recovery is refused because an unresolved bundle still holds the domain.
    pub const HELD_BY_UNRESOLVED_BUNDLE: &str = "config.recovery.held_by_unresolved_bundle";

    /// The set could not be applied as one version and the attempt to undo the
    /// part that was written also failed, so the configuration is genuinely
    /// inconsistent. This is deliberately its own code: it is strictly worse than
    /// a plain failure and a caller must not be told "unavailable".
    pub const PARTIAL_APPLICATION_ROLLBACK_FAILED: &str =
        "config.recovery.partial_application_rollback_failed";

    /// At least one affected domain's validated default does not pass the current
    /// validation, so the scoped default reset cannot be offered.
    pub const DEFAULT_FAILS_VALIDATION: &str = "config.recovery.default_fails_validation";

    /// The confirmation's preview no longer describes the live state.
    ///
    /// Raised when the configuration changed after the preview
    /// was shown, so applying it would overwrite newer, valid configuration. The
    /// user must refresh the preview and confirm again.
    pub const PREVIEW_STALE: &str = "config.recovery.preview_stale";
    /// The action is not available for the bundle as a whole.
    pub const ACTION_NOT_AVAILABLE_FOR_BUNDLE: &str =
        "config.recovery.action_not_available_for_bundle";
}

/// One recorded version found in a domain's history directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedVersion {
    pub revision: u64,
    pub path: PathBuf,
}

/// Lists a domain's recorded versions, newest revision first.
///
/// A recorded version exists only because a document really passed validation and
/// was written by [`crate::service`]'s history recorder. That is what makes it a
/// legitimate "previous valid configuration" rather than an arbitrary old file.
///
/// An unreadable history directory is an error, not an empty list.
/// Returning "no versions" for a directory that could not be read would disable
/// the restore option for the wrong reason and hide the real one.
pub fn recorded_versions(
    paths: &AuraPaths,
    module: ConfigModule,
) -> Result<Vec<RecordedVersion>, SettingsFacadeError> {
    let directory = paths.history_path(module);
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(&directory).map_err(|source| SettingsFacadeError::Io {
        action: "read config history directory",
        path: directory.clone(),
        source,
    })?;
    let prefix = "version-";
    let mut versions = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| SettingsFacadeError::Io {
            action: "read config history entry",
            path: directory.clone(),
            source,
        })?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(rest) = name.strip_prefix(prefix) else { continue };
        // `version-<stamp>-r<revision>.<ext>`
        let Some((_, revision)) = rest.rsplit_once("-r") else { continue };
        let revision = revision.split('.').next().and_then(|value| value.parse::<u64>().ok());
        let Some(revision) = revision else { continue };
        versions.push(RecordedVersion { revision, path: entry.path() });
    }
    versions.sort_by_key(|version| std::cmp::Reverse(version.revision));
    Ok(versions)
}

/// Reads and re-validates one recorded version under the current validator.
///
/// The re-validation is the point: a version that really passed validation *then*
/// may no longer pass *now*, because the domain's rules or the caller's permission
/// may have changed. Applying it anyway would install a document the product
/// currently refuses.
pub fn validate_recorded_version(
    module: ConfigModule,
    version: &RecordedVersion,
    validator: &dyn ModuleConfigValidator,
) -> Result<ModuleConfigDocument, String> {
    let bytes = fs::read(&version.path).map_err(|error| error.to_string())?;
    let document = parse_document(module, &bytes).map_err(|error| error.to_string())?;
    validate_base_document(&document).map_err(|error| error.to_string())?;
    validate_builtin_module_document(module, &document).map_err(|error| error.to_string())?;
    validator.validate(module, &document).map_err(|error| error.to_string())?;
    Ok(document)
}

/// Encodes the domain's validated default.
///
/// The default is produced through the same encoder and then re-parsed and
/// validated, so "reset to default" installs a document that passes the domain's
/// own rules — the "first use may create a default that passes the same validation"
/// rule the user confirmed.
pub fn validated_default(
    module: ConfigModule,
    validator: &dyn ModuleConfigValidator,
) -> Result<(ModuleConfigDocument, Vec<u8>), String> {
    let document = ModuleConfigDocument::empty();
    let bytes = encode_document(module, &document).map_err(|error| error.to_string())?;
    let document = parse_document(module, &bytes).map_err(|error| error.to_string())?;
    validate_base_document(&document).map_err(|error| error.to_string())?;
    validate_builtin_module_document(module, &document).map_err(|error| error.to_string())?;
    validator.validate(module, &document).map_err(|error| error.to_string())?;
    Ok((document, bytes))
}

/// The settings keys that name something outside their own domain.
///
/// These are the references a restore or reset can invalidate: an id that pointed
/// at a model profile, a credential, a tool, a workspace or a server. The list is
/// a *name* list — it never carries a value — and it is what the preview shows as
/// "references needing rebinding".
pub const CROSS_DOMAIN_REFERENCE_KEYS: &[&str] = &[
    "default_model_profile_id",
    "default_provider_id",
    "default_endpoint_id",
    "credential_id",
    "credential_revision",
    "fixed_tool_ids",
    "model_profile_id",
    "provider_id",
    "endpoint_id",
    "servers",
    "workspace_id",
    "session_id",
];

/// The reference keys present in a document, as a sorted name list.
#[must_use]
pub fn cross_domain_references(document: &ModuleConfigDocument) -> Vec<String> {
    let mut found = document
        .values
        .keys()
        .filter(|key| CROSS_DOMAIN_REFERENCE_KEYS.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    found.sort();
    found.dedup();
    found
}

/// The fault classification for a domain, as a stable code.
#[must_use]
pub fn fault_code(error: &SettingsFacadeError) -> &'static str {
    DomainFaultKind::of(error).code()
}

/// A bounded description of what restoring would lose.
///
/// Names the value fields that would change, by key only. It deliberately does not
/// include values: a preview is shown to a user and, for a domain that may hold a
/// reference or a secret-shaped value, echoing values is exactly what the
/// confidentiality boundary forbids.
#[must_use]
pub fn loss_of(
    current: Option<&ModuleConfigDocument>,
    target: &ModuleConfigDocument,
) -> Vec<String> {
    let Some(current) = current else {
        // No snapshot at all: the whole document is being installed.
        return vec![
            "the domain is unavailable, so the restored document replaces no live values"
                .to_owned(),
        ];
    };
    let mut changed = current
        .values
        .keys()
        .filter(|key| current.values.get(*key) != target.values.get(*key))
        .cloned()
        .collect::<Vec<_>>();
    let mut added = target
        .values
        .keys()
        .filter(|key| !current.values.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    changed.sort();
    added.sort();
    let mut loss = Vec::new();
    if !changed.is_empty() {
        loss.push(format!("changed: {}", changed.join(", ")));
    }
    if !added.is_empty() {
        loss.push(format!("added: {}", added.join(", ")));
    }
    if loss.is_empty() {
        loss.push("no value differs from the current document".to_owned());
    }
    loss
}

/// The revision a recovery result must carry.
///
/// Always `current + 1`: recovery uses a **new** revision and never rewinds the
/// file's, so every other writer's compare-and-swap stays meaningful.
pub fn next_revision(current: Option<u64>) -> Result<u64, ConfigRecoveryError> {
    next_revision_for(ConfigModule::Aura, current)
}

/// The next revision for a specific module.
///
/// A domain with no snapshot starts from 0, so its first recovered revision is 1 —
/// the same revision a first-use default would carry.
pub fn next_revision_for(
    module: ConfigModule,
    current: Option<u64>,
) -> Result<u64, ConfigRecoveryError> {
    let current = current.unwrap_or(0);
    current
        .checked_add(1)
        .ok_or(ConfigRecoveryError::Unavailable { module, reason_code: codes::NO_CURRENT_SNAPSHOT })
}

/// Serializes a recovery outcome set for the projection, deterministically.
///
/// Used so the projection's bytes are a function of the outcome alone.
pub fn encode_outcomes(outcomes: &[RecoveryOutcome]) -> Result<Vec<u8>, serde_json::Error> {
    let map = outcomes
        .iter()
        .map(|outcome| (outcome.module.key().to_owned(), outcome.clone()))
        .collect::<BTreeMap<_, _>>();
    serde_json::to_vec(&map)
}

/// The moment a plan was computed, for the projection.
#[must_use]
pub fn planned_at() -> UtcTimestamp {
    UtcTimestamp::now()
}

/// Reads the settings-key names that a document's *values* reference.
///
/// A narrow helper for the preview: it reports only the key names, never values.
#[must_use]
pub fn reference_names(document: &ModuleConfigDocument) -> Vec<String> {
    cross_domain_references(document)
}

/// The file a recovery writes for one domain.
#[must_use]
pub fn recovery_target_path(paths: &AuraPaths, module: ConfigModule) -> PathBuf {
    paths.config_path(module)
}

/// A path-free label for one history version, for a log line.
#[must_use]
pub fn version_label(version: &RecordedVersion) -> String {
    format!("r{}", version.revision)
}

/// Whether a `Path` is inside a domain's history directory.
///
/// Used to assert that a recovery never writes outside managed storage.
#[must_use]
pub fn is_managed_history_path(paths: &AuraPaths, module: ConfigModule, path: &Path) -> bool {
    path.starts_with(paths.history_path(module))
}

/// Binds every member's current bytes/revision, the restore set and offered actions.
#[must_use]
pub fn bundle_preview_identity(plan: &BundleRecoveryPlan) -> String {
    let mut members: Vec<_> =
        plan.domains.iter().map(|p| (p.module.key(), &p.preview_id)).collect();
    members.sort();
    let mut restore = plan.consistent_set.clone();
    restore.sort();
    let facts =
        serde_json::json!({ "members": members, "restore": restore, "options": plan.options });
    let encoded =
        aura_domain_common::jcs::canonical_json_jcs(&facts).expect("recovery facts are valid JSON");
    format!("bundle:{}", aura_domain_common::jcs::CanonicalHashV1::sha256(&encoded).hex())
}
