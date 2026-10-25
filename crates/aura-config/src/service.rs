mod history;
mod input;
use input::ensure_config_size;
pub(crate) use input::read_config_file;
#[cfg(test)]
use input::read_config_file_after_metadata;
mod recovery_apply;
mod recovery_journal;
mod recovery_plan;
mod recovery_publication;
mod recovery_reset;
mod reload;
mod validation;
use validation::parse_and_validate;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::AtomicBool,
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aura_domain_common::{ContentHash, UtcTimestamp};
use aura_windows_platform::{AtomicReplaceError, atomic_replace_file};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use ulid::Ulid;

use crate::document::{contains_secret_field_marker, document_contains_secret_fields};
use crate::health::{ConfigHealthRegistry, DomainFaultKind, DomainHealth, degraded_summary};
use crate::ownership::{DomainClaim, DomainClaimBusy, DomainClaimGuard, DomainClaimRegistry};
use crate::prompt::prompt_snapshot_from_document;
use crate::validate_builtin_module_document;
use crate::{
    AuraPaths, AuraPathsError, BaseModuleConfigValidator, ConfigModule, ConfigValidationError,
    DocumentCodecError, ModuleConfigDocument, ModuleConfigValidator, PromptSettingsSnapshot,
    encode_document, parse_document, validate_base_document,
};

const HISTORY_LIMIT: usize = 5;
const HISTORY_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Maximum encoded size of one file managed as a `SettingsFacade` module.
///
/// This applies before parsing and before any facade-owned write. Internal
/// transaction journals are not settings module files and therefore do not use
/// this per-module limit.
pub const SETTINGS_FILE_MAX_BYTES: u64 = 2 * 1024 * 1024;
pub const SETTINGS_FILE_TOO_LARGE_CODE: &str = "config.settings.file_too_large";

/// How many durable joint-commit records are kept.
///
/// Bounded deliberately: the records are evidence for a cross-domain restore, and
/// only the most recent provable version is realistically restorable. Keeping a
/// bounded window avoids unbounded growth on a long-lived process while leaving
/// enough history for a version that is a few commits old.
const JOINT_COMMIT_RECORD_LIMIT: usize = 8;
/// Stable code for a domain that could not be opened and has no snapshot.
pub const DOMAIN_UNAVAILABLE_CODE: &str = "config.domain.unavailable";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigFaultPoint {
    AfterTempSync,
    BeforeReplace,
    AfterReplace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigBundleFaultPoint {
    AfterFirstReplace,
}

/// Where a domain's currently-served snapshot came from.
///
/// Serializable because a recovery outcome reports it to the user, and the
/// outcome is part of a projection. The wire form is the stable snake_case key, so
/// a renderer can name the source without a second mapping table.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSource {
    Default,
    File,
    LastKnownGood,
}

impl ConfigSource {
    /// A bounded, renderer-safe key for this source.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::File => "file",
            Self::LastKnownGood => "last_known_good",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConfigSnapshot {
    pub module: ConfigModule,
    pub document: Arc<ModuleConfigDocument>,
    pub content_hash: ContentHash,
    pub loaded_at: UtcTimestamp,
    pub source: ConfigSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigEventKind {
    Changed,
    Recovered,
    Rejected,
}

#[derive(Clone, Debug)]
pub struct ConfigEvent {
    pub module: ConfigModule,
    pub kind: ConfigEventKind,
    pub revision: u64,
    pub message: String,
    pub occurred_at: UtcTimestamp,
}

#[derive(Clone, Debug)]
pub struct ConfigModuleBundleEntry {
    pub module: ConfigModule,
    pub expected_revision: u64,
    pub replacement_values: Option<BTreeMap<String, serde_json::Value>>,
}

/// An interrupted bundle journal that could not be converged.
///
/// Carried as data rather than as a facade-open failure so a stuck bundle is
/// visible without preventing the process from starting. The message is the real
/// cause, kept verbatim; the code is the bounded classification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundleJournalFailure {
    pub path: PathBuf,
    pub code: &'static str,
    pub message: String,
}

impl BundleJournalFailure {
    fn new(path: PathBuf, error: &SettingsFacadeError) -> Self {
        Self { path, code: DomainFaultKind::of(error).code(), message: error.to_string() }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedConfigBundle {
    pub transaction_id: String,
    pub entries: Vec<PreparedConfigBundleEntry>,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedConfigBundleEntry {
    pub module: ConfigModule,
    pub before_revision: u64,
    pub before_content_hash: String,
    pub candidate: ModuleConfigDocument,
}

#[derive(Debug, Error)]
pub enum SettingsFacadeError {
    #[error(transparent)]
    Paths(#[from] AuraPathsError),
    #[error("failed to {action} {path}: {source}")]
    Io { action: &'static str, path: PathBuf, source: std::io::Error },
    #[error("failed to decode or encode {module:?}: {source}")]
    Codec { module: ConfigModule, source: DocumentCodecError },
    #[error("configuration file for {module:?} exceeds the {limit_bytes} byte safety limit")]
    FileTooLarge { module: ConfigModule, limit_bytes: u64 },
    #[error(
        "configuration file for {module:?} exceeds the {limit_bytes} byte safety limit and no legitimate last-known-good snapshot exists"
    )]
    FileTooLargeWithLkg { module: ConfigModule, limit_bytes: u64 },
    #[error("configuration domain {module:?} is degraded: {message}")]
    DomainUnavailable { module: ConfigModule, message: String },
    #[error("configuration validation failed for {module:?}: {source}")]
    Validation { module: ConfigModule, source: ConfigValidationError },
    #[error(
        "config revision conflict for {module:?}: expected {expected}, current revision is {actual}"
    )]
    RevisionConflict { module: ConfigModule, expected: u64, actual: u64 },
    #[error(
        "external config revision for {module:?} must increase monotonically: current {current}, candidate {candidate}"
    )]
    NonMonotonicRevision { module: ConfigModule, current: u64, candidate: u64 },
    #[error("config revision for {module:?} cannot advance beyond u64::MAX")]
    RevisionExhausted { module: ConfigModule },
    #[error(transparent)]
    AtomicReplace(#[from] AtomicReplaceError),
    #[error("config fault injected at {0:?}")]
    FaultInjected(ConfigFaultPoint),
    #[error("configuration lock was poisoned: {0}")]
    LockPoisoned(&'static str),
    #[error("failed to start configuration watcher: {0}")]
    Watch(#[from] notify::Error),
    #[error("config bundle must contain at least one replacement")]
    EmptyBundle,
    #[error("config bundle contains duplicate module {0:?}")]
    DuplicateBundleModule(ConfigModule),
    #[error("failed to encode or decode config bundle journal {path}: {source}")]
    BundleJournal { path: PathBuf, source: serde_json::Error },
    #[error(
        "config bundle recovery conflict for {module:?}: current file is neither the frozen before image nor the committed after image"
    )]
    BundleRecoveryConflict { module: ConfigModule },
    #[error("config bundle fault injected at {0:?}")]
    BundleFaultInjected(ConfigBundleFaultPoint),
    #[error("configuration bundle {transaction_id} is already pending runtime visibility")]
    BundlePending { transaction_id: String },
    #[error("configuration bundle is not the active pending transaction")]
    BundleNotPending,
    #[error("configuration bundle transaction id must not be blank")]
    BlankBundleTransactionId,
    /// Another operation owns a domain this request needs.
    ///
    /// A refusal rather than a queue: the caller is told which domains are busy
    /// and returns an honest failure instead of waiting behind a domain that may
    /// be stuck in real file I/O.
    #[error("configuration domain is owned by another operation: {message}")]
    DomainBusy { message: String },
    /// An unresolved cross-domain bundle still includes this domain, so
    /// its snapshot cannot be treated as a fact yet.
    #[error("configuration bundle {transaction_id} is unresolved and still includes this domain")]
    BundleUnresolved { transaction_id: String },
}

impl SettingsFacadeError {
    #[must_use]
    pub const fn stable_code(&self) -> Option<&'static str> {
        match self {
            Self::FileTooLarge { .. } | Self::FileTooLargeWithLkg { .. } => {
                Some(SETTINGS_FILE_TOO_LARGE_CODE)
            }
            Self::DomainUnavailable { .. } => Some(DOMAIN_UNAVAILABLE_CODE),
            _ => None,
        }
    }

    #[must_use]
    pub const fn module(&self) -> Option<ConfigModule> {
        match self {
            Self::Codec { module, .. }
            | Self::FileTooLarge { module, .. }
            | Self::FileTooLargeWithLkg { module, .. }
            | Self::Validation { module, .. }
            | Self::RevisionConflict { module, .. }
            | Self::NonMonotonicRevision { module, .. }
            | Self::RevisionExhausted { module }
            | Self::DuplicateBundleModule(module)
            | Self::BundleRecoveryConflict { module }
            | Self::DomainUnavailable { module, .. } => Some(*module),
            _ => None,
        }
    }
}

struct ModuleConfigService {
    history_dir: PathBuf,
    module: ConfigModule,
    path: PathBuf,
    snapshot: RwLock<Arc<ConfigSnapshot>>,
    validator: Arc<dyn ModuleConfigValidator>,
}

impl ModuleConfigService {
    fn open(
        paths: &AuraPaths,
        module: ConfigModule,
        validator: Arc<dyn ModuleConfigValidator>,
    ) -> Result<(Self, Option<ConfigEvent>), SettingsFacadeError> {
        let path = paths.config_path(module);
        let history_dir = paths.history_path(module);
        fs::create_dir_all(&history_dir).map_err(|source| SettingsFacadeError::Io {
            action: "create config history directory",
            path: history_dir.clone(),
            source,
        })?;

        let exists = path.try_exists().map_err(|source| SettingsFacadeError::Io {
            action: "inspect config file",
            path: path.clone(),
            source,
        })?;
        let (document, bytes, source, recovery) = if exists {
            let current = match read_config_file(module, &path) {
                Ok(bytes) => Some(bytes),
                Err(SettingsFacadeError::FileTooLarge { .. }) => None,
                Err(error) => return Err(error),
            };
            let parsed = match current.as_deref() {
                Some(bytes) => parse_and_validate(module, bytes, validator.as_ref()),
                None => Err(SettingsFacadeError::FileTooLarge {
                    module,
                    limit_bytes: SETTINGS_FILE_MAX_BYTES,
                }),
            };
            match parsed {
                Ok(document) => {
                    let current = current.expect("validated startup bytes are present");
                    (document, current, ConfigSource::File, None)
                }
                Err(original_error) => {
                    if matches!(
                        module,
                        ConfigModule::Memory
                            | ConfigModule::Hooks
                            | ConfigModule::Mcp
                            | ConfigModule::Skills
                    ) {
                        // Memory stays disabled until the user explicitly repairs its input.
                        // So do Hooks and MCP; an older registry could run a Hook or
                        // reach a server the failed edit was removing. And Skills, whose
                        // older choices could offer one the failed edit untrusted.
                        return Err(original_error);
                    }
                    let oversized = current.is_none();
                    let backed_up = if let Some(current) = current.as_deref() {
                        let contains_secret = match parse_document(module, current) {
                            Ok(document) => document_contains_secret_fields(&document),
                            Err(_) => contains_secret_field_marker(current),
                        };
                        if contains_secret {
                            false
                        } else {
                            backup_invalid(&history_dir, module, current)?;
                            true
                        }
                    } else {
                        false
                    };
                    let last_known_good =
                        history_dir.join(format!("last-known-good.{}", module.extension()));
                    let recovered =
                        read_config_file(module, &last_known_good).ok().and_then(|bytes| {
                            parse_and_validate(module, &bytes, validator.as_ref())
                                .ok()
                                .map(|document| (document, bytes))
                        });
                    let (document, bytes, source) = if let Some((document, bytes)) = recovered {
                        (document, bytes, ConfigSource::LastKnownGood)
                    } else {
                        // An existing file is user configuration, even when it
                        // is invalid. Never replace it with empty defaults or
                        // register those defaults as a legitimate backup.
                        return Err(original_error);
                    };
                    if !oversized {
                        atomic_write(&path, &bytes, None)?;
                    }
                    let event = ConfigEvent {
                        module,
                        kind: ConfigEventKind::Recovered,
                        revision: document.revision,
                        message: if oversized {
                            format!(
                                "Oversized startup configuration was left untouched and the runtime recovered from {source:?}."
                            )
                        } else if backed_up {
                            format!(
                                "Invalid startup configuration was backed up and recovered from {source:?}."
                            )
                        } else {
                            format!(
                                "A potentially secret-bearing startup configuration was recovered from {source:?} without copying it to history."
                            )
                        },
                        occurred_at: UtcTimestamp::now(),
                    };
                    (document, bytes, source, Some(event))
                }
            }
        } else {
            let document = ModuleConfigDocument::empty();
            let bytes = encode(module, &document)?;
            let document = parse_and_validate(module, &bytes, validator.as_ref())?;
            atomic_write(&path, &bytes, None)?;
            (document, bytes, ConfigSource::Default, None)
        };

        record_valid_history(&history_dir, module, document.revision, &bytes)?;
        let snapshot = Arc::new(snapshot(module, document, &bytes, source));
        Ok((
            Self { history_dir, module, path, snapshot: RwLock::new(snapshot), validator },
            recovery,
        ))
    }

    fn snapshot(&self) -> Result<Arc<ConfigSnapshot>, SettingsFacadeError> {
        self.snapshot
            .read()
            .map_err(|_| SettingsFacadeError::LockPoisoned("module snapshot"))
            .map(|snapshot| Arc::clone(&snapshot))
    }

    /// Installs an already-encoded, already-validated recovered document.
    ///
    /// Used only by the recovery entry. The caller has already written the
    /// bytes and recorded them in history under the same revision, so this swaps
    /// the in-memory snapshot to exactly what is on disk rather than re-encoding a
    /// second time (which could differ and would then disagree with the file).
    fn replace(&self, installed: Arc<ConfigSnapshot>) -> Result<(), SettingsFacadeError> {
        let mut guard = self
            .snapshot
            .write()
            .map_err(|_| SettingsFacadeError::LockPoisoned("module snapshot"))?;
        *guard = installed;
        Ok(())
    }

    fn update<F>(
        &self,
        expected_revision: u64,
        fault: Option<ConfigFaultPoint>,
        mutate: F,
    ) -> Result<Arc<ConfigSnapshot>, SettingsFacadeError>
    where
        F: FnOnce(&mut BTreeMap<String, serde_json::Value>),
    {
        let mut guard = self
            .snapshot
            .write()
            .map_err(|_| SettingsFacadeError::LockPoisoned("module snapshot"))?;
        let current = Arc::clone(&guard);
        if current.document.revision != expected_revision {
            return Err(SettingsFacadeError::RevisionConflict {
                module: self.module,
                expected: expected_revision,
                actual: current.document.revision,
            });
        }
        let mut candidate = (*current.document).clone();
        mutate(&mut candidate.values);
        candidate.revision = expected_revision
            .checked_add(1)
            .ok_or(SettingsFacadeError::RevisionExhausted { module: self.module })?;
        self.validator
            .validate(self.module, &candidate)
            .map_err(|source| SettingsFacadeError::Validation { module: self.module, source })?;
        validate_builtin_module_document(self.module, &candidate)
            .map_err(|source| SettingsFacadeError::Validation { module: self.module, source })?;
        crate::network_proxy::validate_proxy_change(self.module, &current.document, &candidate)
            .map_err(|source| SettingsFacadeError::Validation { module: self.module, source })?;
        let bytes = encode(self.module, &candidate)?;
        atomic_write(&self.path, &bytes, fault)?;
        let updated = Arc::new(snapshot(self.module, candidate, &bytes, ConfigSource::File));
        record_valid_history(&self.history_dir, self.module, updated.document.revision, &bytes)?;
        *guard = Arc::clone(&updated);
        Ok(updated)
    }

    fn reload(&self) -> Result<Option<Arc<ConfigSnapshot>>, SettingsFacadeError> {
        let bytes = read_config_file(self.module, &self.path)?;
        let candidate = parse_and_validate(self.module, &bytes, self.validator.as_ref())?;
        let mut guard = self
            .snapshot
            .write()
            .map_err(|_| SettingsFacadeError::LockPoisoned("module snapshot"))?;
        let current = Arc::clone(&guard);
        let candidate_hash = ContentHash::blake3(&bytes);
        if candidate.revision == current.document.revision && candidate_hash == current.content_hash
        {
            return Ok(None);
        }
        if candidate.revision <= current.document.revision {
            return Err(SettingsFacadeError::NonMonotonicRevision {
                module: self.module,
                current: current.document.revision,
                candidate: candidate.revision,
            });
        }
        let updated = Arc::new(snapshot(self.module, candidate, &bytes, ConfigSource::File));
        record_valid_history(&self.history_dir, self.module, updated.document.revision, &bytes)?;
        *guard = Arc::clone(&updated);
        Ok(Some(updated))
    }
}

/// A written record that a domain set committed **together in one act**.
///
/// This is the durable evidence a cross-domain restore needs.
/// It exists because a per-domain revision number cannot establish coherence —
/// the counters are independent — so without a record naming the whole set, a
/// legitimate joint commit is indistinguishable from unrelated single-domain saves
/// that happen to share a number.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct JointCommitRecord {
    /// The identity of the transaction that committed the set.
    transaction_id: String,
    /// Every member with its committed revision and content digest.
    members: Vec<JointCommitMember>,
    /// When the set committed.
    committed_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct JointCommitMember {
    module: ConfigModule,
    revision: u64,
    content_hash: String,
}

/// A recorded version that **names a whole domain set together**.
///
/// The only evidence that may justify a cross-domain restore.
/// A per-domain revision number cannot serve as that evidence — the counters are
/// independent, so equal numbers do not imply a shared commit, and one real save
/// can land different numbers per domain.
#[derive(Clone, Debug)]
struct JointBundleRecord {
    /// The revision the whole set shares, as recorded by the joint act.
    revision: u64,
    /// Each member with its revision and content digest, so `apply` restores
    /// exactly the previewed set instead of re-selecting each domain's own latest.
    members: Vec<(ConfigModule, u64, String)>,
}

pub struct SettingsFacade {
    paths: AuraPaths,
    /// One owner per domain. Guarded so a degraded domain can be re-opened, but
    /// never held across a domain's file I/O.
    services: Mutex<BTreeMap<ConfigModule, Arc<ModuleConfigService>>>,
    /// The validators the facade was opened with, so a re-open uses the exact
    /// same validation the startup path used.
    validators: BTreeMap<ConfigModule, Arc<dyn ModuleConfigValidator>>,
    health: ConfigHealthRegistry,
    startup_events: Vec<ConfigEvent>,
    subscribers: Mutex<Vec<Sender<ConfigEvent>>>,
    /// Which operation currently owns which domain.
    ///
    /// This replaces the process-wide `pending_bundle_transaction` mutex that
    /// used to be held across a whole domain's file I/O. The registry is asked
    /// for a **complete** claim set before an operation touches anything, and the
    /// set is released by the guard when the operation really ends. Disjoint
    /// domains therefore never conflict, while a same-domain save, re-open or
    /// recovery still has exactly one owner.
    claims: Arc<DomainClaimRegistry>,
    /// The one active cross-domain transaction, if any.
    ///
    /// This is now a registry keyed by transaction identity, so
    /// **disjoint** transactions coexist while intersecting ones are refused. The
    /// claim registry still decides admission; this records which identities are
    /// live so a conflicting caller can be told who holds the overlap.
    transactions: Mutex<TransactionRegistry>,
    /// Interrupted bundle journals that could not be converged, recorded rather
    /// than returned so a stuck bundle cannot stop the process from starting.
    bundle_journal_failures: Mutex<Vec<BundleJournalFailure>>,
    /// The recovery state determines *responsibility* before any domain
    /// repairs itself.
    recovery: Mutex<RecoveryDisposition>,
}

/// The domain set and identity of the live cross-domain transaction.
///
/// The guard is what actually owns the domains; this record only carries it so
/// the ownership survives past the call that prepared the transaction. Dropping
/// it — which `end_transaction` does by replacing the option — releases the whole
/// set at once.
struct BundleReservation {
    transaction_id: String,
    /// The domains this transaction took, so a conflicting transaction can be told
    /// which domains actually overlap rather than merely that *something* is live.
    modules: Vec<ConfigModule>,
    /// The guard that releases the whole set when the transaction ends.
    #[allow(dead_code)]
    claim_guard: DomainClaimGuard,
}

/// Every live cross-domain transaction, keyed by identity.
///
/// This replaced a single `Mutex<Option<BundleReservation>>`
/// slot plus a single `Mutex<Option<String>>` transaction id. With one slot, a
/// prepared `Ui`/`Diagnostics` bundle made a **completely unrelated** bundle —
/// say `Commands`/`Browser` — fail with `BundlePending`, even though their domain
/// sets are disjoint and the claim registry would have admitted both.
///
/// Ownership is unchanged: the `DomainClaimGuard` inside each entry still owns the
/// domains, and `DomainClaimRegistry::try_claim` is still what decides whether two
/// transactions may coexist (disjoint sets may; intersecting sets may not). This
/// map only records which identities are live so `activate`/`abort` can find the
/// right one and a conflicting caller can be told who holds the overlap.
#[derive(Default)]
struct TransactionRegistry {
    live: BTreeMap<String, BundleReservation>,
}

impl TransactionRegistry {
    /// The live transaction whose domain set overlaps `modules`, if any.
    ///
    /// A disjoint live transaction is deliberately **not** a conflict: that is the
    /// whole point of per-domain ownership, and the claim registry has already
    /// decided whether the claim itself succeeds.
    fn conflict_for(&self, modules: &[ConfigModule]) -> Option<&BundleReservation> {
        self.live
            .values()
            .find(|reservation| reservation.modules.iter().any(|held| modules.contains(held)))
    }
}

/// What recovery has decided about this process's configuration.
///
/// Recovery state must be assigned **before** the domains act. A known
/// bundle failure names a consistent set of domains that are held under control
/// until the bundle is judged; a scan that could not be completed is *unknown*,
/// which is a different fact from "no domains are affected". Conflating the two
/// is how an unreadable manifest directory used to read as an empty affected set.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct RecoveryDisposition {
    /// Domains whose bundle could not be judged, and which therefore must not
    /// serve a "Ready" health or repair their own LKG/default yet.
    held: Vec<ConfigModule>,
    /// Journals that could not be converged or even read, kept verbatim.
    failures: Vec<BundleJournalFailure>,
    /// Whether the affected set itself could be established.
    ///
    /// `false` means the manifest directory or one of its entries could not be
    /// read, so the scope is *unknown*. Unknown is protected by the user's
    /// confirmed rule: keep view/stop/repair, suspend new calls that depend on
    /// the suspect configuration, and never report the uncertain config as Ready.
    scope_known: bool,
}

/// The read-only verdict of judging the interrupted bundle journals.
///
/// This is deliberately a **pure judgement** — scanning
/// and decoding happen here, and nothing is written. It exists so responsibility
/// can be established before `ModuleConfigService::open` performs any LKG or
/// default recovery, because that recovery writes to disk.
///
/// `held` is the set of domains that must not repair themselves yet:
///
/// * a journal that decoded and named its domains contributes exactly those
///   domains — a *known* scope;
/// * a journal that could not be decoded, or a manifest directory that could not
///   be scanned, contributes **every** domain, because the affected set itself is
///   unknown. Unknown is not the empty set; conflating them is how an unreadable
///   manifest used to read as "nothing is affected".
struct RecoveryJudgement {
    /// Domains that must not perform their own LKG/default recovery yet.
    held: Vec<ConfigModule>,
    /// The journals that decoded and can be converged, with the domains they name.
    convergable: Vec<(PathBuf, Vec<ConfigModule>)>,
    /// Every failure found while judging, kept verbatim as the primary cause.
    failures: Vec<BundleJournalFailure>,
    /// Whether the affected set could be established at all.
    scope_known: bool,
    /// A fault to attribute to held domains, so the reason is the real one.
    fault: Option<SettingsFacadeError>,
}

impl RecoveryJudgement {
    /// The real error a held domain is unavailable for.
    ///
    /// This must be a **bundle-classified** error, not a
    /// generic `DomainUnavailable`. A held domain is not unavailable because its
    /// own file was unreadable — it is unavailable because a cross-domain save
    /// that may include it could not be judged. Reporting the generic variant
    /// would classify the fault as `Io` and lose the real reason from both the
    /// health record and the user-facing notice.
    ///
    /// The transaction identity is the journal's own, so a reader can tell which
    /// interrupted save holds this domain.
    fn hold_error(&self, _module: ConfigModule) -> SettingsFacadeError {
        SettingsFacadeError::BundleUnresolved { transaction_id: self.transaction_id() }
    }

    /// The identity of the bundle that holds these domains.
    ///
    /// The journal path is the real identity of an interrupted save: it is what
    /// a retry converges and what a reader can point at. When several failed the
    /// first is named, and every failure is kept verbatim in `failures`.
    fn transaction_id(&self) -> String {
        self.failures
            .first()
            .map(|failure| failure.path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unresolved configuration bundle".to_owned())
    }

    /// The human-readable reason, for events that quote the real failure.
    fn reason(&self) -> String {
        self.fault
            .as_ref()
            .map(ToString::to_string)
            .or_else(|| self.failures.first().map(|failure| failure.message.clone()))
            .unwrap_or_else(|| "an unresolved configuration bundle".to_owned())
    }
}

/// Judges every interrupted cross-domain bundle journal without writing.
///
/// This is the responsibility-assignment step that must precede any single-domain
/// disk write. It reads the manifest directory and decodes each record; it never
/// converges, deletes or rewrites anything. Convergence is a separate, later act
/// (`converge_bundle_journal`) so that a domain which belongs to a half-committed
/// group is not repaired before the group is judged.
fn judge_pending_bundle_transactions(
    paths: &AuraPaths,
    validators: &BTreeMap<ConfigModule, Arc<dyn ModuleConfigValidator>>,
) -> RecoveryJudgement {
    let directory = config_bundle_transaction_dir(paths);
    let journals = match scan_bundle_journals(&directory) {
        Ok(journals) => journals,
        Err(error) => {
            // The scope itself could not be established, so every domain is held.
            return RecoveryJudgement {
                held: ConfigModule::ALL.to_vec(),
                convergable: Vec::new(),
                failures: vec![BundleJournalFailure::new(directory.clone(), &error)],
                scope_known: false,
                fault: Some(error),
            };
        }
    };

    let mut held = Vec::new();
    let mut convergable = Vec::new();
    let mut failures = Vec::new();
    let mut scope_known = true;
    let mut fault: Option<SettingsFacadeError> = None;
    let superseded = recovery_reset::superseded_paths(paths, validators, &journals);
    for journal_path in journals {
        if superseded.contains(&journal_path) || recovery_reset::is_live(&journal_path) {
            continue;
        }
        match read_bundle_journal_domains(paths, validators, &journal_path) {
            Ok((_record, modules)) => {
                // A *valid* journal holds its domains
                // too, during the window between judgement and settlement.
                //
                // This is the point of assigning responsibility before any disk
                // write. `ModuleConfigService::open` performs LKG/default recovery
                // and writes, and that recovery would manufacture a "current"
                // input matching neither the frozen before nor the after image.
                // The bundle must then be validated against the bytes that were
                // really there when the save was interrupted — which only exist if
                // nothing rewrote them first.
                //
                // So the members of an un-settled bundle are opened by
                // convergence, not by the startup loop, and the startup loop must
                // not touch them.
                held.extend(modules.iter().copied());
                convergable.push((journal_path, modules));
            }
            Err((error, modules)) => {
                failures.push(BundleJournalFailure::new(journal_path, &error));
                if modules.is_empty() {
                    // Undecodable: it names nothing, which is not the same as
                    // "no domain is affected". The scope is unknown, so every
                    // domain is held rather than none.
                    scope_known = false;
                    fault = Some(error);
                } else {
                    fault.get_or_insert_with(|| SettingsFacadeError::DomainUnavailable {
                        module: modules[0],
                        message: error.to_string(),
                    });
                    held.extend(modules);
                }
            }
        }
    }
    if !scope_known {
        held = ConfigModule::ALL.to_vec();
    }
    held.sort_unstable();
    held.dedup();
    RecoveryJudgement { held, convergable, failures, scope_known, fault }
}

/// Whether an operation may proceed against a domain while recovery holds it.
///
/// A held domain is one whose cross-domain bundle could not be judged. Its file
/// may be neither the before nor the after image, so:
///
/// * reads of the *held* domain itself are refused — the runtime has no fact it
///   can honestly serve for it;
/// * status, stop and configuration repair stay reachable, which is what the
///   user's confirmed rule requires;
/// * every domain the bundle does not name is untouched.
fn recovery_hold_error(
    module: ConfigModule,
    disposition: &RecoveryDisposition,
) -> Option<SettingsFacadeError> {
    if disposition.held.contains(&module) {
        let failure = disposition
            .failures
            .iter()
            .find(|failure| failure.code == DomainFaultKind::BundleRecovery.code());
        return Some(SettingsFacadeError::DomainUnavailable {
            module,
            message: failure.map_or_else(
                || "an unresolved configuration bundle includes this domain".to_owned(),
                |failure| failure.message.clone(),
            ),
        });
    }
    None
}

impl SettingsFacade {
    pub fn open(paths: AuraPaths) -> Result<Arc<Self>, SettingsFacadeError> {
        Self::open_with_validators(paths, BTreeMap::new())
    }

    /// The current service for one domain, if it has one.
    ///
    /// The returned `Arc` is a real owner: the caller can do its bounded I/O with
    /// the services lock released, so one slow domain cannot block another.
    fn service_for(
        &self,
        module: ConfigModule,
    ) -> Result<Arc<ModuleConfigService>, SettingsFacadeError> {
        self.services
            .lock()
            .map_err(|_| SettingsFacadeError::LockPoisoned("config domain services"))?
            .get(&module)
            .map(Arc::clone)
            .ok_or_else(|| self.domain_unavailable(module))
    }

    /// Converges every interrupted cross-domain bundle journal, one at a time.
    ///
    /// This used to run before any domain was opened and returned on the
    /// first failure, so one unparseable journal stopped the whole facade. Each
    /// journal is now converged on its own; a journal that cannot be converged
    /// degrades only the domains it names, and the domains it does not name keep
    /// working.
    ///
    /// A journal that cannot be **decoded** names no domain at all — there is no
    /// readable record of what it intended — so it degrades nothing and the
    /// facade still opens. That is the honest reading: an undecodable journal is
    /// not evidence that any particular domain is broken. It is reported
    /// through [`SettingsFacade::pending_bundle_journal_failures`] so the
    /// condition is visible rather than swallowed.
    ///
    /// A journal that *does* decode and names domains, but whose convergence
    /// fails, degrades exactly those domains.
    ///
    /// Either way the journal is **left on disk**. Deleting it would destroy the
    /// only record of an interrupted publication, and rewriting it with a
    /// recovery note would overwrite the original failure with a later,
    /// different fact. The domain's health keeps the real error, the journal
    /// keeps the real intent, and the next attempt retries the same convergence.
    pub fn recover_pending_bundle_transactions(&self) -> Result<(), SettingsFacadeError> {
        // A retry re-establishes **current** facts.
        //
        // The previous implementation consumed only `convergable` from the new
        // judgement and copied `held`/`scope_known`/`failures` forward from the
        // state the facade happened to be in. A retry that *newly discovered* an
        // undecodable journal therefore withdrew the services (the unknown branch
        // ran) while still publishing `scope_known = true`, `held = 0` and
        // `failures = 0` — the failure was invisible and the scope was reported
        // known when it was not.
        //
        // The new scan is now the authority for this pass: it replaces the
        // disposition before any recovery side effect.
        let judgement = judge_pending_bundle_transactions(&self.paths, &self.validators);
        self.install_judgement(&judgement);
        self.settle_judged_bundle_transactions(judgement)
    }

    /// Installs a judgement as the live recovery disposition.
    ///
    /// This is the "responsibility becomes the actual state" step. It runs before
    /// any recovery write, so a domain held by a newly discovered problem cannot
    /// be observed as available, and a newly discovered fault cannot be reported
    /// as no fault.
    ///
    /// History is preserved rather than overwritten: a fault that has been
    /// repaired stops being live responsibility but stays in the record, and a
    /// failure from a *different* transaction is never cleared by this one.
    fn install_judgement(&self, judgement: &RecoveryJudgement) {
        for module in &judgement.held {
            // A domain under live responsibility must stop serving a snapshot it
            // cannot justify, and must record why.
            self.health.mark_degraded(*module, &judgement.hold_error(*module));
            self.drop_service(*module);
        }
        if let Ok(mut disposition) = self.recovery.lock() {
            disposition.held = judgement.held.clone();
            disposition.scope_known = judgement.scope_known;
            // The failures are the union of this scan's findings and the records
            // already held for other transactions, so one transaction's failure
            // cannot erase another's responsibility.
            let mut failures = judgement.failures.clone();
            for existing in &disposition.failures {
                if !failures.iter().any(|failure| failure.path == existing.path) {
                    failures.push(existing.clone());
                }
            }
            failures.sort_by(|left, right| left.path.cmp(&right.path));
            failures.dedup_by(|left, right| left.path == right.path);
            disposition.failures = failures;
        }
    }

    /// Settles the journals a judgement found, then releases what is really done.
    ///
    /// Release is decided **per domain, over the
    /// complete set of live responsibility**, never by removing a domain from a
    /// single set.
    ///
    /// * An **unknown** scope settles nothing. Its overlapping component set may
    ///   include any domain, so no member may be written, activated or reopened
    ///   while it stands — otherwise a valid record would "prove" a domain is
    ///   fine while another responsibility still covers it.
    /// * A domain is released only when **no** surviving record names it *and* its
    ///   real service is ready. A single-journal `retain` could not express that:
    ///   a domain named by two records would be released by the first.
    /// * A record is deleted only after its members are settled and their events
    ///   published. Deleting first would lose the only recovery record if reopen
    ///   or publication then failed.
    fn settle_judged_bundle_transactions(
        &self,
        judgement: RecoveryJudgement,
    ) -> Result<(), SettingsFacadeError> {
        let mut failures = judgement.failures.clone();
        // Which records still name which domains. Seeded from the judged set; a
        // record is removed only when it has been settled.
        let mut live: Vec<(PathBuf, Vec<ConfigModule>)> = Vec::new();
        // Records whose members are settled and published, awaiting deletion.
        let mut settled: Vec<(PathBuf, Vec<ConfigModule>)> = Vec::new();
        if !judgement.scope_known {
            // Settle nothing while the scope is unknown. The records stay on disk
            // and every domain stays under protection.
            for (path, modules) in judgement.convergable {
                failures.push(BundleJournalFailure::new(
                    path.clone(),
                    &SettingsFacadeError::BundleUnresolved {
                        transaction_id: path.to_string_lossy().into_owned(),
                    },
                ));
                live.push((path, modules));
            }
            self.publish_settlement(live, failures, false);
            return Ok(());
        }

        for (journal_path, judged_modules) in judgement.convergable {
            match converge_bundle_journal(&self.paths, &self.validators, &journal_path) {
                Ok(converged) => {
                    // The record is settled on disk. Its members may now open —
                    // they were held out of the startup loop, so they have no
                    // service and must be opened from the converged bytes.
                    let mut reopened_ok = true;
                    for module in converged {
                        if let Err(error) = self.reopen_domain(module) {
                            self.health.mark_degraded(module, &error);
                            self.drop_service(module);
                            failures.push(BundleJournalFailure::new(journal_path.clone(), &error));
                            reopened_ok = false;
                        }
                    }
                    if reopened_ok
                        && let Err(error) = recovery_journal::record_joint(self, &journal_path)
                    {
                        failures.push(BundleJournalFailure::new(journal_path.clone(), &error));
                        reopened_ok = false;
                    }
                    // Only a fully settled record leaves the live set. If any
                    // member failed to reopen, the record keeps its
                    // responsibility so the domain is not released on a promise.
                    if !reopened_ok {
                        live.push((journal_path, judged_modules));
                    } else {
                        settled.push((journal_path, judged_modules));
                    }
                }
                Err((error, modules)) => {
                    let modules = if modules.is_empty() { judged_modules } else { modules };
                    failures.push(BundleJournalFailure::new(journal_path.clone(), &error));
                    for module in &modules {
                        self.health.mark_degraded(*module, &error);
                        self.drop_service(*module);
                    }
                    live.push((journal_path, modules));
                }
            }
        }

        // The disposition is published **before** any journal is
        // removed, so a reader never sees a domain released while its record still
        // exists, and a crash between the two leaves the journal (the safe
        // direction) rather than a settled-looking state with no evidence.
        self.publish_settlement(live.clone(), failures.clone(), true);

        // Only now, with every member re-read and its event published, is the
        // record deleted. A deletion that fails keeps the record live, so the next
        // start examines it again instead of silently losing the description.
        let mut still_live = live;
        for (journal_path, modules) in settled {
            match remove_settled_bundle_journal(&journal_path) {
                Ok(()) => {}
                Err(error) => {
                    failures.push(BundleJournalFailure::new(journal_path.clone(), &error));
                    still_live.push((journal_path, modules));
                }
            }
        }
        if !still_live.is_empty() || !failures.is_empty() {
            self.publish_settlement(still_live, failures, true);
        }
        Ok(())
    }

    /// Publishes the settled disposition once, at the end.
    ///
    /// The held set is computed **from the live records**, so it is the union of
    /// every surviving responsibility rather than the residue of a sequence of
    /// removals. A domain is held if any live record names it, or if the scope is
    /// unknown.
    fn publish_settlement(
        &self,
        live: Vec<(PathBuf, Vec<ConfigModule>)>,
        failures: Vec<BundleJournalFailure>,
        scope_known: bool,
    ) {
        let mut held: Vec<ConfigModule> = Vec::new();
        for (_path, modules) in &live {
            held.extend(modules.iter().copied());
        }
        held.sort_unstable();
        held.dedup();
        let held = if scope_known { held } else { ConfigModule::ALL.to_vec() };

        if let Ok(mut recorded) = self.bundle_journal_failures.lock() {
            let mut merged = failures.clone();
            for existing in recorded.iter() {
                if !merged.iter().any(|failure| failure.path == existing.path) {
                    merged.push(existing.clone());
                }
            }
            merged.sort_by(|left, right| left.path.cmp(&right.path));
            merged.dedup_by(|left, right| left.path == right.path);
            *recorded = merged;
        }
        if let Ok(mut disposition) = self.recovery.lock() {
            disposition.held = held.clone();
            disposition.scope_known = scope_known;
            disposition.failures = failures;
        }
        // A released domain serves again only if its real service is there. A
        // domain that is no longer held but whose service was withdrawn must not
        // be reported Ready on a snapshot nobody rebuilt.
        for module in ConfigModule::ALL {
            if !held.contains(&module)
                && self.services.lock().map(|all| all.contains_key(&module)).unwrap_or(false)
            {
                self.health.clear_bundle_hold(module);
            }
        }
    }

    /// Re-opens one domain that was held out of the startup loop.
    ///
    /// A held domain was never opened, so there is no snapshot to reload. This
    /// runs the same [`ModuleConfigService::open`] the startup path uses — same
    /// validator, same LKG/default rules — now that its bundle has been settled.
    fn reopen_domain(&self, module: ConfigModule) -> Result<(), SettingsFacadeError> {
        let validator = self
            .validators
            .get(&module)
            .cloned()
            .unwrap_or_else(|| Arc::new(BaseModuleConfigValidator));
        let (service, recovery) = ModuleConfigService::open(&self.paths, module, validator)?;
        let current = service.snapshot()?;
        self.health.record_recovery(module, current.source, current.document.revision);
        self.services
            .lock()
            .map_err(|_| SettingsFacadeError::LockPoisoned("config domain services"))?
            .insert(module, Arc::new(service));
        if let Some(event) = recovery {
            self.publish(event)?;
        }
        Ok(())
    }

    /// Holds every domain when the affected set itself could not be established.
    ///
    /// Drops one domain's in-memory owner.
    ///
    /// A domain whose durability is unknown must stop serving a snapshot it
    /// cannot justify. The service is removed only to swap the map entry, never
    /// while any domain's file I/O is in progress.
    fn drop_service(&self, module: ConfigModule) {
        if let Ok(mut services) = self.services.lock() {
            services.remove(&module);
        }
    }

    /// Interrupted bundle journals that could not be converged.
    ///
    /// Reported instead of returned as a facade-open failure: a stuck bundle
    /// must not stop the process from starting, but it must not be invisible
    /// either. Callers surface these through the existing diagnostic path.
    #[must_use]
    pub fn pending_bundle_journal_failures(&self) -> Vec<BundleJournalFailure> {
        self.bundle_journal_failures.lock().map(|failures| failures.clone()).unwrap_or_default()
    }

    /// Opens every domain, isolating each one's failure.
    ///
    /// This used to be `for module in ConfigModule::ALL {... ?; }`, so the
    /// first domain whose file could not be opened, decoded or validated aborted
    /// the entire facade — all fifteen domains, plus every session and control
    /// path that needs settings. Each domain is now opened independently and its
    /// real failure is recorded against that domain alone; the facade still
    /// fails only when the *layout itself* cannot be prepared, because with no
    /// directories there is no domain to be independent of.
    pub fn open_with_validators(
        paths: AuraPaths,
        validators: BTreeMap<ConfigModule, Arc<dyn ModuleConfigValidator>>,
    ) -> Result<Arc<Self>, SettingsFacadeError> {
        paths.ensure_layout()?;
        cleanup_orphan_temps(&paths.config_root)?;
        // Establish *responsibility* before any domain
        // repairs itself.
        //
        // Judging the journals is a read-only act: it scans the manifest
        // directory and decodes each record, and it writes nothing. Doing it here
        // — before the per-domain open loop — is what makes the ordering correct.
        // Previously the loop ran first, and `ModuleConfigService::open` writes to
        // disk (LKG or default recovery), so a domain that was really one half of
        // an interrupted bundle had its input silently rewritten *before* the
        // facade decided whether that bundle could be judged. An unknown scope
        // must not first alter input that may belong to a half-committed group.
        //
        // A held domain is therefore not opened at all: it has no defensible
        // snapshot, so it stays degraded and unreachable until recovery settles
        // its scope. `converge_bundle_journal` is what actually writes, and it
        // runs after this judgement.
        let judgement = judge_pending_bundle_transactions(&paths, &validators);
        let mut services = BTreeMap::new();
        let mut startup_events = Vec::new();
        let mut health = Vec::with_capacity(ConfigModule::ALL.len());
        for module in ConfigModule::ALL {
            // A domain named by an unjudged bundle, or covered by an unknown
            // scope, must not repair itself yet. It is recorded as degraded with
            // the bundle's own fault and given no service.
            if judgement.held.contains(&module) {
                let error = judgement.hold_error(module);
                health.push(DomainHealth::degraded(module, &error));
                startup_events.push(ConfigEvent {
                    module,
                    kind: ConfigEventKind::Rejected,
                    revision: 0,
                    message: format!(
                        "Configuration domain is held by an unresolved bundle ({}): {}; its input was left untouched.",
                        DomainFaultKind::of(&error).code(),
                        judgement.reason()
                    ),
                    occurred_at: UtcTimestamp::now(),
                });
                continue;
            }
            let validator = validators
                .get(&module)
                .cloned()
                .unwrap_or_else(|| Arc::new(BaseModuleConfigValidator));
            match ModuleConfigService::open(&paths, module, validator) {
                Ok((service, recovery)) => {
                    let current = service.snapshot()?;
                    health.push(DomainHealth::ready(
                        module,
                        current.source,
                        current.document.revision,
                    ));
                    services.insert(module, Arc::new(service));
                    if let Some(event) = recovery {
                        startup_events.push(event);
                    }
                }
                Err(error) => {
                    // This domain has no snapshot at all. Every other domain is
                    // still opened; a caller that asks for *this* module gets the
                    // original error, and a caller that asks for another module
                    // is unaffected.
                    health.push(DomainHealth::degraded(module, &error));
                    startup_events.push(ConfigEvent {
                        module,
                        kind: ConfigEventKind::Rejected,
                        revision: 0,
                        message: format!(
                            "Configuration domain could not be opened ({}); other domains are unaffected.",
                            DomainFaultKind::of(&error).code()
                        ),
                        occurred_at: UtcTimestamp::now(),
                    });
                }
            }
        }
        let health = ConfigHealthRegistry::new(health);
        // The responsibility decided by the read-only
        // judgement is installed **before** convergence runs, so no reader can
        // observe a window in which a held domain looks available. Convergence
        // then narrows the held set as each journal is settled.
        let initial_held = judgement.held.clone();
        let initial_scope_known = judgement.scope_known;
        let initial_failures = judgement.failures.clone();
        let facade = Arc::new(Self {
            paths,
            services: Mutex::new(services),
            validators,
            health,
            startup_events,
            subscribers: Mutex::new(Vec::new()),
            claims: Arc::new(DomainClaimRegistry::new()),
            transactions: Mutex::new(TransactionRegistry::default()),
            bundle_journal_failures: Mutex::new(initial_failures),
            recovery: Mutex::new(RecoveryDisposition {
                held: initial_held,
                failures: judgement.failures.clone(),
                scope_known: initial_scope_known,
            }),
        });
        // A bundle journal names the domains it actually contains. The journals
        // were already judged above (read-only), and the health registry already
        // records every held domain from that judgement. This pass performs the
        // writes, and it runs only after every *unheld* domain is open and every
        // held domain has been left untouched. A journal that cannot be converged
        // degrades only the domains it names.
        facade.settle_judged_bundle_transactions(judgement)?;
        Ok(facade)
    }

    /// The health of one configuration domain.
    #[must_use]
    pub fn health(&self, module: ConfigModule) -> DomainHealth {
        self.health.health(module)
    }

    /// Every domain's health, in `ConfigModule::ALL` order.
    #[must_use]
    pub fn health_all(&self) -> Vec<DomainHealth> {
        self.health.all()
    }

    /// The domains that are currently degraded, in module order.
    #[must_use]
    pub fn degraded_domains(&self) -> Vec<ConfigModule> {
        self.health.degraded()
    }

    /// A bounded, path-free summary of the degraded set.
    #[must_use]
    pub fn degraded_summary(&self) -> String {
        degraded_summary(&self.health.all())
    }

    /// Whether this domain currently has a real in-memory snapshot.
    #[must_use]
    pub fn is_available(&self, module: ConfigModule) -> bool {
        self.services.lock().map(|services| services.contains_key(&module)).unwrap_or(false)
    }

    #[must_use]
    pub fn paths(&self) -> &AuraPaths {
        &self.paths
    }

    #[must_use]
    pub fn startup_events(&self) -> &[ConfigEvent] {
        &self.startup_events
    }

    pub fn snapshot(
        &self,
        module: ConfigModule,
    ) -> Result<Arc<ConfigSnapshot>, SettingsFacadeError> {
        self.service_for(module)?.snapshot()
    }

    pub fn prompt_snapshot(&self) -> Result<PromptSettingsSnapshot, SettingsFacadeError> {
        let snapshot = self.snapshot(ConfigModule::Prompt)?;
        prompt_snapshot_from_document(&snapshot.document).map_err(|source| {
            SettingsFacadeError::Validation { module: ConfigModule::Prompt, source }
        })
    }

    pub fn tool_settings_snapshot(
        &self,
    ) -> Result<aura_tool_contract::ToolSettingsSnapshotV1, SettingsFacadeError> {
        let snapshot = self.snapshot(ConfigModule::Tools)?;
        crate::tool_settings_snapshot_from_document(&snapshot.document).map_err(|source| {
            SettingsFacadeError::Validation { module: ConfigModule::Tools, source }
        })
    }

    pub fn update<F>(
        &self,
        module: ConfigModule,
        expected_revision: u64,
        mutate: F,
    ) -> Result<Arc<ConfigSnapshot>, SettingsFacadeError>
    where
        F: FnOnce(&mut BTreeMap<String, serde_json::Value>),
    {
        self.update_with_fault(module, expected_revision, None, mutate)
    }

    pub fn update_with_fault<F>(
        &self,
        module: ConfigModule,
        expected_revision: u64,
        fault: Option<ConfigFaultPoint>,
        mutate: F,
    ) -> Result<Arc<ConfigSnapshot>, SettingsFacadeError>
    where
        F: FnOnce(&mut BTreeMap<String, serde_json::Value>),
    {
        self.update_inner(module, expected_revision, fault, mutate, None)
    }

    /// An ordinary save with an observation taken **while the domain is owned**.
    ///
    /// This seam exists because the earlier evidence was
    /// post-hoc. `ConfigFaultPoint::AfterReplace` fires inside the save and then
    /// unwinds, so a test that signalled from there was waiting *after* the claim
    /// had been released — a hold outside the window it claimed to measure, which
    /// the old global lock would also have passed.
    ///
    /// `hold` runs with the domain's exclusive claim still held, so a test that
    /// parks here is genuinely inside the ownership window: an unrelated domain
    /// making progress at that moment is real evidence of per-domain isolation.
    #[doc(hidden)]
    pub fn update_with_hold<F, H>(
        &self,
        module: ConfigModule,
        expected_revision: u64,
        mutate: F,
        hold: H,
    ) -> Result<Arc<ConfigSnapshot>, SettingsFacadeError>
    where
        F: FnOnce(&mut BTreeMap<String, serde_json::Value>),
        H: FnOnce() + 'static,
    {
        self.update_inner(module, expected_revision, None, mutate, Some(Box::new(hold)))
    }

    fn update_inner<F>(
        &self,
        module: ConfigModule,
        expected_revision: u64,
        fault: Option<ConfigFaultPoint>,
        mutate: F,
        hold: Option<Box<dyn FnOnce()>>,
    ) -> Result<Arc<ConfigSnapshot>, SettingsFacadeError>
    where
        F: FnOnce(&mut BTreeMap<String, serde_json::Value>),
    {
        // An ordinary domain write claims **only its own domain**, and it
        // claims nothing at all while another domain is doing its own bounded
        // file I/O. Before R1 this took a process-wide transaction mutex and held
        // it across `service.update(...)`, so one domain stuck in a real save
        // blocked every other domain's write.
        //
        // The cross-domain concern is still honoured, without a global lock: a
        // domain that an unresolved bundle includes is refused here, because the
        // one fact a save depends on is that this domain's revision is a real
        // fact. Claiming the unavailable domain is impossible precisely because
        // it has no service, so the check is explicit rather than incidental.
        self.ensure_not_held_by_recovery(module)?;
        let _claim = self.claim_one(module, DomainClaim::Exclusive)?;
        // The observation runs with the claim held, so it really overlaps the
        // ownership window this domain owns.
        if let Some(hold) = hold {
            hold();
        }
        let service = self.service_for(module)?;
        let updated = match service.update(expected_revision, fault, mutate) {
            Ok(updated) => updated,
            Err(error) => {
                // The domain keeps serving its previous valid snapshot; the
                // failed attempt is recorded without clearing the domain.
                self.health.record_write_failure(module, &error);
                return Err(error);
            }
        };
        self.health.record_snapshot(module, updated.source, updated.document.revision);
        self.publish(ConfigEvent {
            module,
            kind: ConfigEventKind::Changed,
            revision: updated.document.revision,
            message: "Configuration snapshot changed.".to_owned(),
            occurred_at: UtcTimestamp::now(),
        })?;
        Ok(updated)
    }

    /// Claims one domain, waiting for that domain alone.
    ///
    /// An ordinary save or re-open of one domain **waits for that
    /// domain** rather than being refused. Two writers of the same domain with
    /// the same expected revision must produce one commit and one revision
    /// conflict — the compare-and-swap the old global lock used to provide — and
    /// a busy refusal would instead turn a legitimate retry into a spurious
    /// failure. The wait is on a single domain and the waiter holds nothing else,
    /// so no unrelated domain is blocked and no wait cycle can form.
    fn claim_one(
        &self,
        module: ConfigModule,
        claim: DomainClaim,
    ) -> Result<DomainClaimGuard, SettingsFacadeError> {
        // The `AURA_S05R2_GLOBAL_LOCK_COUNTERFACTUAL` env switch that
        // used to live here has been **removed from product code**.
        //
        // It restored the pre-R1 process-wide lock, which was useful for showing that the
        // isolation criteria reject the old design — but a switch that can re-introduce a
        // global lock is a product-path mutation, and "the variable is not set in any
        // shipping configuration" is not a structural guarantee of unreachability. The
        // counterfactual is reproducible by temporarily editing this function, not by setting a
        // variable.
        match claim {
            DomainClaim::Exclusive => self.claims.claim_serially(module).map_err(busy_error),
            DomainClaim::Shared => self.claims.try_claim([(module, claim)]).map_err(busy_error),
        }
    }

    /// Claims a whole set all at once, or refuses.
    ///
    /// A multi-domain transaction never waits: it either gets its complete set or
    /// reports which domains are busy. Waiting for a set would require holding
    /// part of it, which is how lock-order deadlocks are built.
    fn claim_set(
        &self,
        claims: impl IntoIterator<Item = (ConfigModule, DomainClaim)>,
    ) -> Result<DomainClaimGuard, SettingsFacadeError> {
        self.claims.try_claim(claims).map_err(busy_error)
    }

    /// Refuses an operation on a domain that an unresolved bundle still holds.
    fn ensure_not_held_by_recovery(&self, module: ConfigModule) -> Result<(), SettingsFacadeError> {
        let disposition = self
            .recovery
            .lock()
            .map_err(|_| SettingsFacadeError::LockPoisoned("config recovery disposition"))?;
        match recovery_hold_error(module, &disposition) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// The domains an unresolved bundle currently holds, in module order.
    #[must_use]
    pub fn recovery_held_domains(&self) -> Vec<ConfigModule> {
        self.recovery.lock().map(|disposition| disposition.held.clone()).unwrap_or_default()
    }

    /// Whether recovery could establish *which* domains an unresolved bundle
    /// affects. `false` is the honest "unknown", not the empty set.
    #[must_use]
    pub fn recovery_scope_known(&self) -> bool {
        self.recovery.lock().map(|disposition| disposition.scope_known).unwrap_or(false)
    }

    /// The validator this facade was opened with for one domain.
    fn validator_for(&self, module: ConfigModule) -> Arc<dyn ModuleConfigValidator> {
        self.validators.get(&module).cloned().unwrap_or_else(|| Arc::new(BaseModuleConfigValidator))
    }

    /// Writes the durable record that this set committed together.
    ///
    /// Bounded like everything else here: only the most recent
    /// [`JOINT_COMMIT_RECORD_LIMIT`] records are kept, so a long-lived process
    /// cannot accumulate unbounded evidence. Pruning removes the *oldest*, so the
    /// most recent provable version is the one most likely to still be present.
    fn write_joint_commit_record(
        &self,
        prepared: &PreparedConfigBundle,
        updated: &BTreeMap<ConfigModule, Arc<ConfigSnapshot>>,
    ) -> Result<(), SettingsFacadeError> {
        let members = prepared
            .entries
            .iter()
            .filter_map(|entry| {
                updated.get(&entry.module).map(|snapshot| JointCommitMember {
                    module: entry.module,
                    revision: snapshot.document.revision,
                    content_hash: snapshot.content_hash.to_string(),
                })
            })
            .collect::<Vec<_>>();
        // A single-domain "bundle" proves nothing cross-domain and needs no record;
        // writing one would imply a joint act that did not happen.
        if members.len() < 2 {
            return Ok(());
        }
        let record = JointCommitRecord {
            transaction_id: prepared.transaction_id.clone(),
            members,
            committed_at: UtcTimestamp::now(),
        };
        let directory = joint_commit_record_dir(&self.paths);
        fs::create_dir_all(&directory).map_err(|source| SettingsFacadeError::Io {
            action: "create joint commit record directory",
            path: directory.clone(),
            source,
        })?;
        let path = directory.join(format!("{}.json", prepared.transaction_id));
        let bytes = serde_json::to_vec_pretty(&record).map_err(|_| SettingsFacadeError::Io {
            action: "encode joint commit record",
            path: path.clone(),
            source: std::io::Error::other("joint commit record is not encodable"),
        })?;
        atomic_write(&path, &bytes, None)?;
        prune_joint_commit_records(&directory);
        Ok(())
    }

    /// The most recent joint record that names **every** requested module.
    ///
    /// Read-only. Returns the record's members with their digests so the caller can
    /// bind a restore to what really committed.
    fn latest_joint_commit_record(&self, modules: &[ConfigModule]) -> Option<JointCommitRecord> {
        let directory = joint_commit_record_dir(&self.paths);
        let mut paths = fs::read_dir(&directory)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
            .collect::<Vec<_>>();
        paths.sort();
        let wanted: BTreeSet<ConfigModule> = modules.iter().copied().collect();
        if wanted.is_empty() {
            return None;
        }
        let mut best: Option<JointCommitRecord> = None;
        for path in paths {
            let Ok(bytes) = read_internal_file(&path) else {
                continue;
            };
            let Ok(record) = serde_json::from_slice::<JointCommitRecord>(&bytes) else {
                continue;
            };
            let named: BTreeSet<ConfigModule> =
                record.members.iter().map(|member| member.module).collect();
            // The record must name the *whole* set: a partial record does not prove
            // that the members it omits were part of the same act.
            if !wanted.is_subset(&named) {
                continue;
            }
            let newer = best
                .as_ref()
                .is_none_or(|current| record.committed_at.value() >= current.committed_at.value());
            if newer {
                best = Some(record);
            }
        }
        best
    }

    /// Finds a real joint record covering **every** requested module.
    ///
    /// This is the only evidence that may justify a cross-domain
    /// restore. An interrupted bundle journal is a genuine joint record — it names
    /// its complete domain set and carries each member's frozen facts from one act.
    /// A per-domain revision number cannot serve as that evidence, because the
    /// counters are independent: equal numbers do not imply a shared commit.
    ///
    /// When no journal covers the whole set the answer is `None`, and the caller
    /// disables the restore rather than inventing coherence.
    fn joint_bundle_record_for(&self, modules: &[ConfigModule]) -> Option<JointBundleRecord> {
        // A committed set leaves a durable joint record, which is the strongest
        // evidence and the normal case for a completed bundle.
        if let Some(record) = self.latest_joint_commit_record(modules) {
            let revision = record.members.iter().map(|member| member.revision).max()?;
            let members = record
                .members
                .iter()
                .map(|member| (member.module, member.revision, member.content_hash.clone()))
                .collect::<Vec<_>>();
            return Some(JointBundleRecord { revision, members });
        }
        // Otherwise an *interrupted* bundle journal is also a real joint record: it
        // names its complete domain set and carries each member's frozen facts.
        let directory = config_bundle_transaction_dir(&self.paths);
        let journals = scan_bundle_journals(&directory).ok()?;
        let wanted: std::collections::BTreeSet<ConfigModule> = modules.iter().copied().collect();
        if wanted.is_empty() {
            return None;
        }
        for journal_path in journals {
            let Ok((record, named)) =
                read_bundle_journal_domains(&self.paths, &self.validators, &journal_path)
            else {
                continue;
            };
            let named_set: std::collections::BTreeSet<ConfigModule> =
                named.iter().copied().collect();
            // The record must name the *whole* set: a partial record does not prove
            // that the members it omits were ever part of the same act.
            if !wanted.is_subset(&named_set) {
                continue;
            }
            let revision = record.entries.iter().map(|entry| entry.candidate.revision).max()?;
            let members = record
                .entries
                .iter()
                .filter(|entry| wanted.contains(&entry.module))
                .map(|entry| {
                    let digest = encode(entry.module, &entry.candidate)
                        .map(|bytes| ContentHash::blake3(&bytes).to_string())
                        .unwrap_or_default();
                    (entry.module, entry.candidate.revision, digest)
                })
                .collect::<Vec<_>>();
            return Some(JointBundleRecord { revision, members });
        }
        None
    }

    /// The user's recovery preview for a **cross-domain** recovery. Writes nothing.
    ///
    /// A cross-domain restore must reach **one provable consistent
    /// version**. This plan proves it or disables it:
    ///
    /// * a **consistent set** requires a usable joint commit record naming every
    ///   affected member's actual revision and content; matching independent
    ///   revision counters never proves that the files were committed together;
    /// * when no such record exists, the cross-domain restore is honestly
    ///   disabled with `config.recovery.no_consistent_bundle_version`, and the
    ///   scoped default reset is offered instead — it never staples each domain's
    ///   own last-known-good together and calls the result consistent.
    pub fn plan_bundle_recovery(
        &self,
        modules: &[ConfigModule],
    ) -> Result<crate::recovery::BundleRecoveryPlan, crate::recovery::ConfigRecoveryError> {
        use crate::recovery::{BundleRecoveryPlan, ConfigRecoveryAction, RecoveryOption, codes};

        // Several capabilities can name the same damaged domain. Recovery owns a
        // set of files, not one publication for every fault mentioning that file.
        // Normalize before reading, hashing, claiming or publishing any member.
        let mut unique_modules = modules.to_vec();
        unique_modules.sort();
        unique_modules.dedup();
        let modules = unique_modules.as_slice();
        let superseded = recovery_reset::superseded(self, modules).map_err(|source| {
            crate::recovery::ConfigRecoveryError::Plan {
                module: ConfigModule::Aura,
                source: Box::new(source),
            }
        })?;
        let mut domains = Vec::with_capacity(modules.len());
        for module in modules {
            domains.push(self.bound_domain_in_set(*module, superseded.is_some())?.plan);
        }

        // A *consistent version* must be proven by a real joint
        // record, not by two domains happening to carry the same revision number.
        //
        // The previous implementation intersected the per-domain recoverable
        // revision **numbers** and called the intersection a coherent bundle. That
        // reasoning is invalid: each domain's revision counter is independent
        // (a domain increments its own file), so two wholly unrelated single-domain
        // saves both reaching revision 2 produce an "intersection" of {2} with no
        // shared commit behind it, as a measurement showed. A legitimate whole
        // save can also land different numbers per domain, so the number is not
        // even a necessary condition.
        //
        // The only sound proof is a record that *names the whole set together* —
        // an interrupted bundle journal that covers every affected domain, or a
        // recorded joint commit. `record_valid_history` stores only a revision and
        // its bytes, so no such record exists for an ordinary save; when it is
        // absent the restore is honestly disabled rather than guessed.
        let joint = self.joint_bundle_record_for(modules);
        let consistent_revision = joint.as_ref().map(|record| record.revision);
        let inconsistency_code = if consistent_revision.is_none() {
            Some(codes::NO_CONSISTENT_BUNDLE_VERSION.to_owned())
        } else {
            None
        };
        // The exact set the chosen version restores, so `apply` consumes what the
        // preview showed rather than re-selecting each domain's own latest.
        let consistent_set: Vec<(ConfigModule, u64, String)> =
            joint.as_ref().map(|record| record.members.clone()).unwrap_or_default();

        let held = domains.iter().any(|domain| {
            domain.options.iter().any(|option| {
                option.reason_code.as_deref() == Some(codes::HELD_BY_UNRESOLVED_BUNDLE)
            })
        });

        let mut options = Vec::with_capacity(3);
        if held {
            options.push(RecoveryOption::unavailable_for_plan(
                ConfigRecoveryAction::Retry,
                codes::HELD_BY_UNRESOLVED_BUNDLE,
                "an unresolved configuration bundle still includes one of these domains",
            ));
        } else {
            options.push(RecoveryOption::available_for_plan(ConfigRecoveryAction::Retry));
        }

        match consistent_revision {
            Some(revision) => options.push(RecoveryOption::available_with_detail(
                ConfigRecoveryAction::RestorePreviousValid,
                format!(
                    "all {} affected domains have a recorded version at revision {revision}, so \
                     that revision is one consistent version",
                    domains.len()
                ),
            )),
            None => options.push(RecoveryOption::unavailable_for_plan(
                ConfigRecoveryAction::RestorePreviousValid,
                codes::NO_CONSISTENT_BUNDLE_VERSION,
                "no single revision is recorded by every affected domain, so a consistent previous \
                 version cannot be proven; each domain's own last-known-good must not be stapled \
                 together and called consistent. Use the scoped default reset instead.",
            )),
        }

        // The scoped default reset is offered when every affected domain's default
        // passes current validation, and it resets exactly these domains — never
        // the whole configuration.
        let all_defaults_valid = domains.iter().all(|domain| {
            domain.options.iter().any(|option| {
                option.action == ConfigRecoveryAction::ResetToDefault && option.available
            })
        });
        if all_defaults_valid {
            options.push(RecoveryOption::available_with_detail(
                ConfigRecoveryAction::ResetToDefault,
                format!(
                    "resets exactly the {} affected domains to their validated defaults; no other \
                     configuration domain, credential, session or workspace is touched",
                    domains.len()
                ),
            ));
        } else {
            options.push(RecoveryOption::unavailable_for_plan(
                ConfigRecoveryAction::ResetToDefault,
                "config.recovery.default_fails_validation",
                "at least one affected domain's default does not pass the current validation",
            ));
        }

        let mut plan = BundleRecoveryPlan {
            preview_id: String::new(),
            modules: modules.to_vec(),
            options,
            consistent_revision,
            domains,
            inconsistency_code,
            consistent_set,
        };
        if superseded.is_some() {
            plan.options.retain(|option| option.action == ConfigRecoveryAction::ResetToDefault);
            plan.consistent_set.clear();
            plan.consistent_revision = None;
            if let Some(option) = plan.options.first_mut() {
                option.detail = Some("All listed configuration domains are reset. Existing non-secret settings are lost; credentials, sessions and workspaces remain, but bindings may need to be configured again.".into());
            }
        }
        plan.preview_id = crate::recovery::bundle_preview_identity(&plan);
        if let Some(bindings) = superseded {
            plan.preview_id = recovery_reset::preview(plan.preview_id, &bindings);
        }
        Ok(plan)
    }

    /// Publishes the facts for one domain whose bundle recovery really landed.
    fn finish_domain_recovery(
        &self,
        module: ConfigModule,
        action: crate::recovery::ConfigRecoveryAction,
        confirmed_document: &ModuleConfigDocument,
        confirmed_bytes: &[u8],
    ) -> Result<crate::recovery::RecoveryOutcome, crate::recovery::ConfigRecoveryError> {
        use crate::recovery::{ConfigRecoveryAction, RecoveryOutcome};
        let apply_error = |source: SettingsFacadeError| {
            crate::recovery::ConfigRecoveryError::Apply { module, source: Box::new(source) }
        };
        // Re-read the file that was just written, so the served snapshot is the
        // real committed fact rather than an assumption about it.
        let bytes =
            read_config_file(module, &self.paths.config_path(module)).map_err(apply_error)?;
        if bytes != confirmed_bytes {
            return Err(apply_error(SettingsFacadeError::BundleRecoveryConflict { module }));
        }
        let document = confirmed_document.clone();
        let source = match action {
            ConfigRecoveryAction::RestorePreviousValid => ConfigSource::LastKnownGood,
            ConfigRecoveryAction::ResetToDefault => ConfigSource::Default,
            ConfigRecoveryAction::Retry => ConfigSource::File,
        };
        let snapshot = Arc::new(snapshot(module, document, &bytes, source));
        record_valid_history(
            &self.paths.history_path(module),
            module,
            snapshot.document.revision,
            &bytes,
        )
        .map_err(apply_error)?;
        self.services
            .lock()
            .map_err(|_| crate::recovery::ConfigRecoveryError::Apply {
                module,
                source: Box::new(SettingsFacadeError::LockPoisoned("config domain services")),
            })?
            .insert(
                module,
                Arc::new(ModuleConfigService {
                    history_dir: self.paths.history_path(module),
                    module,
                    path: self.paths.config_path(module),
                    snapshot: std::sync::RwLock::new(Arc::clone(&snapshot)),
                    validator: self.validator_for(module),
                }),
            );
        self.health.record_recovery(module, source, snapshot.document.revision);
        self.drop_recovery_hold(module);
        self.publish(ConfigEvent {
            module,
            kind: ConfigEventKind::Recovered,
            revision: snapshot.document.revision,
            message: "The configuration domain was recovered as part of a consistent set."
                .to_owned(),
            occurred_at: UtcTimestamp::now(),
        })
        .map_err(apply_error)?;
        Ok(RecoveryOutcome {
            module,
            action,
            applied: true,
            revision: Some(snapshot.document.revision),
            source: Some(source),
            reason_code: None,
        })
    }

    /// Releases one domain from the recovery hold, after a real recovery.
    fn drop_recovery_hold(&self, module: ConfigModule) {
        if let Ok(mut disposition) = self.recovery.lock() {
            disposition.held.retain(|held| *held != module);
        }
    }

    pub fn prepare_module_bundle(
        &self,
        transaction_id: String,
        mut entries: Vec<ConfigModuleBundleEntry>,
    ) -> Result<PreparedConfigBundle, SettingsFacadeError> {
        if transaction_id.trim().is_empty() {
            return Err(SettingsFacadeError::BlankBundleTransactionId);
        }
        if entries.is_empty() || entries.iter().all(|entry| entry.replacement_values.is_none()) {
            return Err(SettingsFacadeError::EmptyBundle);
        }
        entries.sort_by_key(|entry| entry.module);
        for pair in entries.windows(2) {
            if pair[0].module == pair[1].module {
                return Err(SettingsFacadeError::DuplicateBundleModule(pair[0].module));
            }
        }
        for entry in &entries {
            self.ensure_not_held_by_recovery(entry.module)?;
        }

        // A cross-domain transaction admits its **complete** domain set
        // once, here, and keeps it for the whole transaction. The set includes
        // the read-only revision dependencies: an entry with no replacement is
        // still a domain whose frozen revision the bundle's before-image refers
        // to, so it is claimed `Shared` rather than ignored.
        let claims = entries
            .iter()
            .map(|entry| {
                let claim = if entry.replacement_values.is_some() {
                    DomainClaim::Exclusive
                } else {
                    DomainClaim::Shared
                };
                (entry.module, claim)
            })
            .collect::<Vec<_>>();
        let claim_guard = self.claim_set(claims)?;

        let services = entries
            .iter()
            .map(|entry| self.service_for(entry.module))
            .collect::<Result<Vec<_>, _>>()?;
        let snapshots =
            services.iter().map(|service| service.snapshot()).collect::<Result<Vec<_>, _>>()?;
        for (entry, snapshot) in entries.iter().zip(&snapshots) {
            if snapshot.document.revision != entry.expected_revision {
                return Err(SettingsFacadeError::RevisionConflict {
                    module: entry.module,
                    expected: entry.expected_revision,
                    actual: snapshot.document.revision,
                });
            }
        }

        let mut prepared_entries = Vec::new();
        for ((entry, snapshot), service) in entries.iter().zip(&snapshots).zip(&services) {
            let Some(values) = entry.replacement_values.clone() else {
                continue;
            };
            let mut candidate = (*snapshot.document).clone();
            candidate.values = values;
            candidate.revision = entry
                .expected_revision
                .checked_add(1)
                .ok_or(SettingsFacadeError::RevisionExhausted { module: entry.module })?;
            service.validator.validate(entry.module, &candidate).map_err(|source| {
                SettingsFacadeError::Validation { module: entry.module, source }
            })?;
            validate_builtin_module_document(entry.module, &candidate).map_err(|source| {
                SettingsFacadeError::Validation { module: entry.module, source }
            })?;
            crate::network_proxy::validate_proxy_change(
                entry.module,
                &snapshot.document,
                &candidate,
            )
            .map_err(|source| SettingsFacadeError::Validation { module: entry.module, source })?;
            let _ = encode(entry.module, &candidate)?;
            prepared_entries.push(PreparedConfigBundleEntry {
                module: entry.module,
                before_revision: entry.expected_revision,
                before_content_hash: snapshot.content_hash.to_string(),
                candidate,
            });
        }
        if prepared_entries.is_empty() {
            return Err(SettingsFacadeError::EmptyBundle);
        }
        let prepared = PreparedConfigBundle {
            transaction_id: transaction_id.clone(),
            entries: prepared_entries,
            created_at: UtcTimestamp::now(),
        };
        validate_prepared_bundle(&prepared)?;
        // The transaction's domain set is reserved from here until `activate`
        // or `abort` really ends it. Handing the guard to the reservation is what
        // makes the ownership survive the call boundary *without* a process-wide
        // lock: an unrelated domain is never in this map, so it is never blocked.
        //
        // `modules` is recorded alongside the guard so a later
        // transaction can be told which domains actually overlap. A disjoint
        // transaction is admitted by the claim registry and must not be refused
        // here merely because *some* transaction is live.
        let modules: Vec<ConfigModule> = prepared.entries.iter().map(|e| e.module).collect();
        self.reserve_transaction(&transaction_id, modules, claim_guard)?;
        Ok(prepared)
    }

    /// Reserves the transaction's whole domain set and its identity.
    fn reserve_transaction(
        &self,
        transaction_id: &str,
        modules: Vec<ConfigModule>,
        claim_guard: DomainClaimGuard,
    ) -> Result<(), SettingsFacadeError> {
        let mut registry = self
            .transactions
            .lock()
            .map_err(|_| SettingsFacadeError::LockPoisoned("config bundle transactions"))?;
        // A transaction that intersects a live one is refused and told which
        // identity holds the overlap. A *disjoint* transaction is admitted: the
        // claim registry has already granted it its own domains, and refusing it
        // here is exactly the global-slot defect this registry replaced.
        if let Some(active) = registry.conflict_for(&modules) {
            return Err(SettingsFacadeError::BundlePending {
                transaction_id: active.transaction_id.clone(),
            });
        }
        registry.live.insert(
            transaction_id.to_owned(),
            BundleReservation { transaction_id: transaction_id.to_owned(), modules, claim_guard },
        );
        Ok(())
    }

    /// Whether a transaction identity is live.
    fn is_transaction_live(&self, transaction_id: &str) -> Result<bool, SettingsFacadeError> {
        Ok(self
            .transactions
            .lock()
            .map_err(|_| SettingsFacadeError::LockPoisoned("config bundle transactions"))?
            .live
            .contains_key(transaction_id))
    }

    /// Ends one transaction, releasing exactly its domain set.
    ///
    /// Only the named transaction is removed.
    /// Another live transaction's record is never deleted by this call, which the
    /// single-slot version could not express — ending anything cleared the slot.
    fn end_transaction(&self, transaction_id: &str) -> Result<(), SettingsFacadeError> {
        let mut registry = self
            .transactions
            .lock()
            .map_err(|_| SettingsFacadeError::LockPoisoned("config bundle transactions"))?;
        if registry.live.remove(transaction_id).is_none() {
            return Err(SettingsFacadeError::BundleNotPending);
        }
        // Dropping the entry drops its `DomainClaimGuard`, releasing exactly this
        // transaction's domains. Other entries keep theirs.
        Ok(())
    }

    pub fn resume_prepared_module_bundle(
        &self,
        prepared: &PreparedConfigBundle,
    ) -> Result<(), SettingsFacadeError> {
        validate_prepared_bundle(prepared)?;
        // Recovery owns the same component admission as preparation: a resumed
        // transaction must not be able to take domains that a live transaction,
        // an ordinary save or another recovery attempt already owns.
        for entry in &prepared.entries {
            self.ensure_not_held_by_recovery(entry.module)?;
        }
        if self.is_transaction_live(&prepared.transaction_id)? {
            // Already resumed: resuming is idempotent for the same identity.
            return Ok(());
        }
        // A live transaction that owns an *overlapping* domain is a conflict; a
        // disjoint one is not, and the claim below is what decides.
        let modules: Vec<ConfigModule> = prepared.entries.iter().map(|e| e.module).collect();
        if let Some(conflict) =
            self.transactions.lock().ok().and_then(|registry| {
                registry.conflict_for(&modules).map(|r| r.transaction_id.clone())
            })
        {
            return Err(SettingsFacadeError::BundlePending { transaction_id: conflict });
        }
        let claims = prepared.entries.iter().map(|entry| (entry.module, DomainClaim::Exclusive));
        let claim_guard = self.claim_set(claims)?;
        self.reserve_transaction(&prepared.transaction_id, modules, claim_guard)
    }

    pub fn publish_prepared_module_bundle(
        &self,
        prepared: &PreparedConfigBundle,
    ) -> Result<(), SettingsFacadeError> {
        self.publish_prepared_module_bundle_with_fault(prepared, None)
    }

    pub fn publish_prepared_module_bundle_with_fault(
        &self,
        prepared: &PreparedConfigBundle,
        fault: Option<ConfigBundleFaultPoint>,
    ) -> Result<(), SettingsFacadeError> {
        validate_prepared_bundle(prepared)?;
        // The transaction already owns these domains. Publishing does its real
        // file I/O **without** holding any other domain's claim: the ownership
        // this transaction has is exactly the ownership it took at prepare time.
        if !self.is_transaction_live(&prepared.transaction_id)? {
            return Err(SettingsFacadeError::BundleNotPending);
        }
        let journal_path = write_config_bundle_transaction(&self.paths, prepared)?;
        for (index, entry) in prepared.entries.iter().enumerate() {
            let candidate_bytes = encode(entry.module, &entry.candidate)?;
            let path = self.paths.config_path(entry.module);
            let current_bytes = read_config_file(entry.module, &path)?;
            let current_hash = ContentHash::blake3(&current_bytes).to_string();
            let candidate_hash = ContentHash::blake3(&candidate_bytes).to_string();
            if current_hash != candidate_hash {
                if current_hash != entry.before_content_hash {
                    return Err(SettingsFacadeError::BundleRecoveryConflict {
                        module: entry.module,
                    });
                }
                atomic_write(&path, &candidate_bytes, None)?;
            }
            if index == 0 && fault == Some(ConfigBundleFaultPoint::AfterFirstReplace) {
                return Err(SettingsFacadeError::BundleFaultInjected(
                    ConfigBundleFaultPoint::AfterFirstReplace,
                ));
            }
        }
        let _ = journal_path;
        Ok(())
    }

    pub fn activate_published_module_bundle(
        &self,
        prepared: &PreparedConfigBundle,
    ) -> Result<BTreeMap<ConfigModule, Arc<ConfigSnapshot>>, SettingsFacadeError> {
        validate_prepared_bundle(prepared)?;
        if !self.is_transaction_live(&prepared.transaction_id)? {
            return Err(SettingsFacadeError::BundleNotPending);
        }

        let services = prepared
            .entries
            .iter()
            .map(|entry| self.service_for(entry.module))
            .collect::<Result<Vec<_>, _>>()?;
        let mut guards = Vec::with_capacity(services.len());
        for service in &services {
            guards.push(
                service
                    .snapshot
                    .write()
                    .map_err(|_| SettingsFacadeError::LockPoisoned("module bundle snapshot"))?,
            );
        }

        let mut updated = BTreeMap::new();
        for ((entry, service), guard) in prepared.entries.iter().zip(&services).zip(&guards) {
            let candidate_bytes = encode(entry.module, &entry.candidate)?;
            let candidate_hash = ContentHash::blake3(&candidate_bytes).to_string();
            let current_snapshot_is_before = guard.document.revision == entry.before_revision
                && guard.content_hash.to_string() == entry.before_content_hash;
            let current_snapshot_is_candidate = guard.document.revision == entry.candidate.revision
                && guard.content_hash.to_string() == candidate_hash;
            if !current_snapshot_is_before && !current_snapshot_is_candidate {
                return Err(SettingsFacadeError::BundleRecoveryConflict { module: entry.module });
            }
            let on_disk_hash = ContentHash::blake3(&read_config_file(
                entry.module,
                &self.paths.config_path(entry.module),
            )?)
            .to_string();
            if on_disk_hash != candidate_hash {
                return Err(SettingsFacadeError::BundleRecoveryConflict { module: entry.module });
            }
            record_valid_history(
                &service.history_dir,
                entry.module,
                entry.candidate.revision,
                &candidate_bytes,
            )?;
            updated.insert(
                entry.module,
                Arc::new(snapshot(
                    entry.module,
                    entry.candidate.clone(),
                    &candidate_bytes,
                    ConfigSource::File,
                )),
            );
        }
        for ((entry, mut guard), _) in prepared.entries.iter().zip(guards).zip(&services) {
            *guard = Arc::clone(
                updated.get(&entry.module).expect("prepared module has an activated snapshot"),
            );
            self.health.record_snapshot(entry.module, guard.source, guard.document.revision);
        }
        // Record that this set really committed **together**
        // before the journal is removed.
        //
        // This is what makes a later cross-domain restore provable. Without it the
        // only evidence left after a successful bundle is each domain's own
        // history, and per-domain revision numbers cannot establish that a set was
        // one act — which is precisely why the equal-number rule was wrong. The
        // record names the complete set with each member's revision and content
        // digest, so a restore can be checked against what really committed.
        self.write_joint_commit_record(prepared, &updated)?;
        let journal_path = config_bundle_transaction_dir(&self.paths)
            .join(format!("{}.json", prepared.transaction_id));
        if journal_path.exists() {
            fs::remove_file(&journal_path).map_err(|source| SettingsFacadeError::Io {
                action: "finish config bundle transaction",
                path: journal_path,
                source,
            })?;
        }
        self.end_transaction(&prepared.transaction_id)?;
        for snapshot in updated.values() {
            self.publish(ConfigEvent {
                module: snapshot.module,
                kind: ConfigEventKind::Changed,
                revision: snapshot.document.revision,
                message: "Configuration bundle snapshot changed.".to_owned(),
                occurred_at: UtcTimestamp::now(),
            })?;
        }
        Ok(updated)
    }

    pub fn abort_prepared_module_bundle(
        &self,
        prepared: &PreparedConfigBundle,
    ) -> Result<(), SettingsFacadeError> {
        validate_prepared_bundle(prepared)?;
        if !self.is_transaction_live(&prepared.transaction_id)? {
            return Err(SettingsFacadeError::BundleNotPending);
        }
        for entry in &prepared.entries {
            let current_hash = ContentHash::blake3(&read_config_file(
                entry.module,
                &self.paths.config_path(entry.module),
            )?)
            .to_string();
            if current_hash != entry.before_content_hash {
                return Err(SettingsFacadeError::BundleRecoveryConflict { module: entry.module });
            }
        }
        let journal_path = config_bundle_transaction_dir(&self.paths)
            .join(format!("{}.json", prepared.transaction_id));
        if journal_path.exists() {
            fs::remove_file(&journal_path).map_err(|source| SettingsFacadeError::Io {
                action: "abort config bundle transaction",
                path: journal_path,
                source,
            })?;
        }
        self.end_transaction(&prepared.transaction_id)?;
        Ok(())
    }

    pub fn commit_module_bundle(
        &self,
        entries: Vec<ConfigModuleBundleEntry>,
    ) -> Result<BTreeMap<ConfigModule, Arc<ConfigSnapshot>>, SettingsFacadeError> {
        self.commit_module_bundle_with_fault(entries, None)
    }

    pub fn commit_module_bundle_with_fault(
        &self,
        entries: Vec<ConfigModuleBundleEntry>,
        fault: Option<ConfigBundleFaultPoint>,
    ) -> Result<BTreeMap<ConfigModule, Arc<ConfigSnapshot>>, SettingsFacadeError> {
        let prepared = self.prepare_module_bundle(format!("ctx_{}", Ulid::new()), entries)?;
        self.publish_prepared_module_bundle_with_fault(&prepared, fault)?;
        self.activate_published_module_bundle(&prepared)
    }

    fn domain_unavailable(&self, module: ConfigModule) -> SettingsFacadeError {
        let health = self.health.health(module);
        SettingsFacadeError::DomainUnavailable {
            module,
            message: health
                .original_message
                .unwrap_or_else(|| "the domain was never opened".to_owned()),
        }
    }

    pub fn subscribe(&self) -> Result<Receiver<ConfigEvent>, SettingsFacadeError> {
        let (sender, receiver) = mpsc::channel();
        self.subscribers
            .lock()
            .map_err(|_| SettingsFacadeError::LockPoisoned("config event subscribers"))?
            .push(sender);
        Ok(receiver)
    }

    /// Starts bounded, per-domain coalescing file watching.
    ///
    /// The single consumer thread that reloaded every affected module
    /// inline lived here and is gone. The implementation is in [`crate::watch`]
    /// so this already-oversized module does not grow another responsibility, and
    /// so the watcher's own bounds are visible in one place.
    pub fn start_watching(self: &Arc<Self>) -> Result<ConfigWatchHandle, SettingsFacadeError> {
        crate::watch::start_watching(self).map(ConfigWatchHandle::from)
    }

    /// The domain that owns one configuration path, if any.
    pub(crate) fn module_for_path(&self, path: &Path) -> Option<ConfigModule> {
        ConfigModule::ALL.into_iter().find(|module| self.paths.config_path(*module) == path)
    }

    fn publish(&self, event: ConfigEvent) -> Result<(), SettingsFacadeError> {
        let mut subscribers = self
            .subscribers
            .lock()
            .map_err(|_| SettingsFacadeError::LockPoisoned("config event subscribers"))?;
        subscribers.retain(|sender| sender.send(event.clone()).is_ok());
        Ok(())
    }
}

/// Turns a busy domain set into an honest refusal.
///
/// The domains are named by their stable keys, never by paths or user input, so
/// the message is safe for a projection or a log.
fn busy_error(busy: DomainClaimBusy) -> SettingsFacadeError {
    SettingsFacadeError::DomainBusy { message: busy.message() }
}

fn validate_prepared_bundle(prepared: &PreparedConfigBundle) -> Result<(), SettingsFacadeError> {
    if prepared.transaction_id.trim().is_empty() {
        return Err(SettingsFacadeError::BlankBundleTransactionId);
    }
    if prepared.entries.is_empty() {
        return Err(SettingsFacadeError::EmptyBundle);
    }
    let mut modules = BTreeSet::new();
    for entry in &prepared.entries {
        if !modules.insert(entry.module) {
            return Err(SettingsFacadeError::DuplicateBundleModule(entry.module));
        }
        if entry.candidate.revision
            != entry
                .before_revision
                .checked_add(1)
                .ok_or(SettingsFacadeError::RevisionExhausted { module: entry.module })?
        {
            return Err(SettingsFacadeError::BundleRecoveryConflict { module: entry.module });
        }
        validate_base_document(&entry.candidate)
            .map_err(|source| SettingsFacadeError::Validation { module: entry.module, source })?;
        validate_builtin_module_document(entry.module, &entry.candidate)
            .map_err(|source| SettingsFacadeError::Validation { module: entry.module, source })?;
    }
    Ok(())
}

/// Handle to the running configuration watcher.
///
/// A thin wrapper over [`crate::watch::ConfigWatcherHandle`] so the
/// watcher's real ownership (notifier plus bounded workers) stays in one module
/// while this type remains the stable public name callers already hold.
pub struct ConfigWatchHandle {
    inner: crate::watch::ConfigWatcherHandle,
}

impl From<crate::watch::ConfigWatcherHandle> for ConfigWatchHandle {
    fn from(inner: crate::watch::ConfigWatcherHandle) -> Self {
        Self { inner }
    }
}

impl std::fmt::Debug for ConfigWatchHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ConfigWatchHandle").finish_non_exhaustive()
    }
}

/// Kept for source compatibility with callers that referenced the old shape.
#[allow(dead_code)]
impl ConfigWatchHandle {
    fn stop_flag(&self) -> &Arc<AtomicBool> {
        self.inner.stop_flag()
    }
}

fn encode(
    module: ConfigModule,
    document: &ModuleConfigDocument,
) -> Result<Vec<u8>, SettingsFacadeError> {
    let bytes = encode_document(module, document)
        .map_err(|source| SettingsFacadeError::Codec { module, source })?;
    ensure_config_size(module, bytes.len() as u64)?;
    Ok(bytes)
}

fn snapshot(
    module: ConfigModule,
    document: ModuleConfigDocument,
    bytes: &[u8],
    source: ConfigSource,
) -> ConfigSnapshot {
    ConfigSnapshot {
        module,
        document: Arc::new(document),
        content_hash: ContentHash::blake3(bytes),
        loaded_at: UtcTimestamp::now(),
        source,
    }
}

/// Where durable joint-commit records live.
///
/// Separate from the transaction journal directory, because
/// these records describe commits that **succeeded** and must outlive the journal
/// that was removed on activation.
fn joint_commit_record_dir(paths: &AuraPaths) -> PathBuf {
    paths.local_root.join("runtime").join("joint-commits")
}

/// Keeps the joint-commit record set bounded.
///
/// The oldest records are removed first, so the most recent provable version
/// survives. A pruning failure is deliberately ignored: losing the *oldest*
/// evidence must never fail a commit that has already succeeded on disk.
fn prune_joint_commit_records(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .collect::<Vec<_>>();
    if paths.len() <= JOINT_COMMIT_RECORD_LIMIT {
        return;
    }
    paths.sort();
    let excess = paths.len() - JOINT_COMMIT_RECORD_LIMIT;
    for path in paths.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
}

fn config_bundle_transaction_dir(paths: &AuraPaths) -> PathBuf {
    paths.config_transaction_dir.join("module-bundles")
}

/// Lists the interrupted bundle journals, refusing to treat an unreadable entry
/// as an absent one.
///
/// The previous implementation used `.filter_map(Result::ok)`, which
/// turns a per-entry read error into "that entry does not exist". That is the
/// "unknown as empty" failure: a journal that really is on disk but whose
/// directory entry could not be read would silently not be recovered, and its
/// domains would be reported Ready on bytes that may be a half-published bundle.
///
/// Here a single unreadable entry makes the whole scope unknown, which the
/// caller handles with the user's confirmed protection rather than by pretending
/// the entry was never there.
fn scan_bundle_journals(directory: &Path) -> Result<Vec<PathBuf>, SettingsFacadeError> {
    fs::create_dir_all(directory).map_err(|source| SettingsFacadeError::Io {
        action: "create config bundle transaction directory",
        path: directory.to_path_buf(),
        source,
    })?;
    let entries = fs::read_dir(directory).map_err(|source| SettingsFacadeError::Io {
        action: "scan config bundle transaction directory",
        path: directory.to_path_buf(),
        source,
    })?;
    let mut journals = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| SettingsFacadeError::Io {
            action: "read config bundle transaction entry",
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "json") {
            journals.push(path);
        }
    }
    journals.sort();
    Ok(journals)
}

fn write_config_bundle_transaction(
    paths: &AuraPaths,
    record: &PreparedConfigBundle,
) -> Result<PathBuf, SettingsFacadeError> {
    let directory = config_bundle_transaction_dir(paths);
    fs::create_dir_all(&directory).map_err(|source| SettingsFacadeError::Io {
        action: "create config bundle transaction directory",
        path: directory.clone(),
        source,
    })?;
    let path = directory.join(format!("{}.json", record.transaction_id));
    let mut bytes = serde_json::to_vec_pretty(record)
        .map_err(|source| SettingsFacadeError::BundleJournal { path: path.clone(), source })?;
    bytes.push(b'\n');
    atomic_write(&path, &bytes, None)?;
    Ok(path)
}

/// Converges one interrupted cross-domain bundle journal and reports every
/// domain the bundle contains.
///
/// The journal names the exact domains whose publication was interrupted.
/// Recovery converges those domains and no others, and it converges them *as a
/// whole* — a bundle whose forward image is partially on disk is completed
/// forward, never reported as a partial success. A journal that cannot be
/// converged is returned as an error together with the modules it names, so the
/// caller degrades only those domains and leaves the rest of the process running.
///
/// The returned module list is the bundle's own membership, not "the domains
/// that happened to need a write". After convergence the whole bundle's after
/// image is the confirmed fact, so the caller must re-read all of them.
///
/// The revision/content-hash check is the same one the interactive path uses: a
/// file that is neither the frozen before image nor the committed candidate is a
/// real conflict, not something to overwrite.
/// Decodes one bundle journal and validates its candidates, **writing nothing**.
///
/// This is the single implementation of "what does this
/// journal say, and is it structurally sound?". The read-only judgement pass uses
/// it to assign responsibility before any domain repairs itself, and
/// [`converge_bundle_journal`] reuses it before performing the writes. One
/// decoder means the held set and the converged set can never disagree about what
/// a journal names.
///
/// Returns the decoded record plus the domains it names in entry order — the
/// domains are computed even for a record that fails validation, so a malformed
/// journal still names what it claimed to touch.
fn read_bundle_journal_domains(
    _paths: &AuraPaths,
    validators: &BTreeMap<ConfigModule, Arc<dyn ModuleConfigValidator>>,
    journal_path: &Path,
) -> Result<(PreparedConfigBundle, Vec<ConfigModule>), (SettingsFacadeError, Vec<ConfigModule>)> {
    let bytes = read_internal_file(journal_path).map_err(|error| (error, Vec::new()))?;
    let recovery = recovery_journal::Record::decode(&bytes, journal_path)
        .map_err(|error| (error, Vec::new()))?;
    let record: PreparedConfigBundle = match recovery {
        Some(record) => record.candidate_set(),
        None => serde_json::from_slice(&bytes).map_err(|source| {
            (
                SettingsFacadeError::BundleJournal { path: journal_path.to_path_buf(), source },
                Vec::new(),
            )
        })?,
    };
    let bundle_modules = record.entries.iter().map(|entry| entry.module).collect::<Vec<_>>();
    let fail = |error: SettingsFacadeError| (error, bundle_modules.clone());
    if record.entries.is_empty() {
        return Err(fail(SettingsFacadeError::EmptyBundle));
    }
    let mut modules = BTreeSet::new();
    for entry in &record.entries {
        if !modules.insert(entry.module) {
            return Err(fail(SettingsFacadeError::DuplicateBundleModule(entry.module)));
        }
        validate_base_document(&entry.candidate)
            .map_err(|source| SettingsFacadeError::Validation { module: entry.module, source })
            .map_err(fail)?;
        validate_builtin_module_document(entry.module, &entry.candidate)
            .map_err(|source| SettingsFacadeError::Validation { module: entry.module, source })
            .map_err(fail)?;
        let fallback = BaseModuleConfigValidator;
        let validator = validators
            .get(&entry.module)
            .map_or(&fallback as &dyn ModuleConfigValidator, Arc::as_ref);
        validator
            .validate(entry.module, &entry.candidate)
            .map_err(|source| SettingsFacadeError::Validation { module: entry.module, source })
            .map_err(fail)?;
    }
    Ok((record, bundle_modules))
}

fn converge_bundle_journal(
    paths: &AuraPaths,
    validators: &BTreeMap<ConfigModule, Arc<dyn ModuleConfigValidator>>,
    journal_path: &Path,
) -> Result<Vec<ConfigModule>, (SettingsFacadeError, Vec<ConfigModule>)> {
    // The decode and structural validation are shared
    // with the read-only judgement pass, so the domains a journal names are
    // computed by exactly one implementation. The judgement pass calls this and
    // stops; convergence continues into the writes below.
    let (record, bundle_modules) = read_bundle_journal_domains(paths, validators, journal_path)?;
    let fail = |error: SettingsFacadeError| (error, bundle_modules.clone());
    let bytes = read_internal_file(journal_path).map_err(fail)?;
    if let Some(recovery) = recovery_journal::Record::decode(&bytes, journal_path).map_err(fail)? {
        if serde_json::to_value(recovery.candidate_set()).ok() != serde_json::to_value(&record).ok()
        {
            return Err(fail(SettingsFacadeError::BundleUnresolved {
                transaction_id: record.transaction_id,
            }));
        }
        recovery.converge(paths).map_err(fail)?;
        return Ok(bundle_modules);
    }
    // Validate the whole record before writing any of it: a bundle that cannot
    // be completed whole must not be half-written by recovery either.
    for entry in &record.entries {
        let candidate_bytes = encode(entry.module, &entry.candidate).map_err(fail)?;
        let path = paths.config_path(entry.module);
        let current_bytes = read_config_file(entry.module, &path).map_err(fail)?;
        let current_hash = ContentHash::blake3(&current_bytes).to_string();
        let candidate_hash = ContentHash::blake3(&candidate_bytes).to_string();
        if current_hash == candidate_hash {
            continue;
        }
        let current = parse_document(entry.module, &current_bytes)
            .map_err(|source| SettingsFacadeError::Codec { module: entry.module, source })
            .map_err(fail)?;
        if current.revision != entry.before_revision || current_hash != entry.before_content_hash {
            return Err(fail(SettingsFacadeError::BundleRecoveryConflict { module: entry.module }));
        }
    }
    for entry in &record.entries {
        let candidate_bytes = encode(entry.module, &entry.candidate).map_err(fail)?;
        let path = paths.config_path(entry.module);
        let current_bytes = read_config_file(entry.module, &path).map_err(fail)?;
        let candidate_hash = ContentHash::blake3(&candidate_bytes).to_string();
        if ContentHash::blake3(&current_bytes).to_string() == candidate_hash {
            continue;
        }
        atomic_write(&path, &candidate_bytes, None).map_err(fail)?;
    }

    // The journal is deliberately **not** removed here.
    //
    // This function performs only the domain writes. Removing the record is the
    // caller's job, and only after every member has been re-read and its recovery
    // event published — otherwise a failure in that later work leaves the
    // configuration converged on disk with no record of what was interrupted. The
    // journal is the only description of the interrupted save, so it outlives the
    // write and is deleted last.
    Ok(bundle_modules)
}

/// Removes one bundle journal once its recovery is fully settled.
///
/// Deletion is separated from convergence so it can happen
/// **after** the members are re-read and their events published. "Already gone" is
/// success — another pass settled it — while any other failure is reported rather
/// than swallowed, because a journal left behind is a record the next start will
/// examine again.
fn remove_settled_bundle_journal(journal_path: &Path) -> Result<(), SettingsFacadeError> {
    match fs::remove_file(journal_path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(SettingsFacadeError::Io {
            action: "finish recovered config bundle transaction",
            path: journal_path.to_path_buf(),
            source,
        }),
    }
}

fn read_internal_file(path: &Path) -> Result<Vec<u8>, SettingsFacadeError> {
    fs::read(path).map_err(|source| SettingsFacadeError::Io {
        action: "read config file",
        path: path.to_path_buf(),
        source,
    })
}

fn atomic_write(
    path: &Path,
    bytes: &[u8],
    fault: Option<ConfigFaultPoint>,
) -> Result<(), SettingsFacadeError> {
    let parent = path.parent().ok_or_else(|| SettingsFacadeError::Io {
        action: "resolve config parent",
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing parent"),
    })?;
    fs::create_dir_all(parent).map_err(|source| SettingsFacadeError::Io {
        action: "create config parent",
        path: parent.to_path_buf(),
        source,
    })?;
    let temporary = parent.join(format!(".aura-{}.tmp", Ulid::new()));
    let mut file =
        OpenOptions::new().create_new(true).write(true).open(&temporary).map_err(|source| {
            SettingsFacadeError::Io {
                action: "create config temporary file",
                path: temporary.clone(),
                source,
            }
        })?;
    file.write_all(bytes).and_then(|()| file.flush()).and_then(|()| file.sync_all()).map_err(
        |source| SettingsFacadeError::Io {
            action: "flush config temporary file",
            path: temporary.clone(),
            source,
        },
    )?;
    drop(file);
    if fault == Some(ConfigFaultPoint::AfterTempSync)
        || fault == Some(ConfigFaultPoint::BeforeReplace)
    {
        return Err(SettingsFacadeError::FaultInjected(fault.expect("fault is present")));
    }
    atomic_replace_file(&temporary, path)?;
    if fault == Some(ConfigFaultPoint::AfterReplace) {
        return Err(SettingsFacadeError::FaultInjected(ConfigFaultPoint::AfterReplace));
    }
    Ok(())
}

fn record_valid_history(
    history_dir: &Path,
    module: ConfigModule,
    revision: u64,
    bytes: &[u8],
) -> Result<(), SettingsFacadeError> {
    history::record(history_dir, module, revision, bytes)
}

/// Writes one history file, tolerating a concurrent writer that got there first.
///
/// This closes a real race rather than a test artifact.
///
/// # The race
///
/// Two `SettingsFacade`s over the same configuration root is a legitimate situation —
/// the isolation tests do exactly what a second entry point would, opening a
/// second facade while the runtime's own facade is live. Both then write the **same**
/// `last-known-good.<ext>` path for a domain, and the write is a read-modify-write that
/// was not synchronised between them.
///
/// `atomic_replace_file` refuses to retry a replace whose target **identity binding**
/// changed underneath it — a correct guard, because retrying would replace a file the
/// caller never inspected. The concurrent facade's write changed exactly that binding, so
/// the guard fired as `AtomicReplaceError::RetryBindingChanged` and the whole save failed
/// even though the file on disk was already correct.
///
/// # What may be retried, and what may not
///
/// There are two different situations behind one error, and only one of them is safe to retry:
///
/// * **the destination already holds exactly these bytes** — the goal is achieved, so the write
///   is done. History entries are written for their content, so a writer that lost the race to
///   an *identical* value has nothing left to do;
/// * **the destination holds something else** — another writer legitimately saved different
///   content, or a newer revision. Retrying would replace a file this call never inspected, and
///   the identity guard refused precisely that. This is **not** retried: the error propagates,
///   the other writer's value stays, and the caller learns its own content did not land.
///
/// The earlier form of this function retried unconditionally, which meant a later attempt could
/// take the destination away from a concurrent writer that had just saved newer content — the
/// guard was respected on the first attempt and then bypassed on the second. That is the defect
/// `a_later_history_write_never_replaces_a_concurrent_writers_newer_content` now measures.
///
/// # The native partial-rename error
///
/// The native error observed under parallel load was `1176`
/// (`ERROR_UNABLE_TO_MOVE_REPLACEMENT`), which `atomic_file` deliberately does **not** retry
/// internally: it is a *partial* rename, so the replacement name may or may not still exist and
/// blindly retrying could replace a file the caller never inspected. That refusal is correct and
/// is **not** weakened here.
///
/// A `1176` is retried **only when the destination is missing or still holds the value this call
/// expected to replace** — i.e. when the partial state is unambiguously this call's own residue
/// and no other writer has published anything. A destination holding a different, complete value
/// is another writer's success and is left alone.
fn write_history_file(path: &Path, bytes: &[u8]) -> Result<(), SettingsFacadeError> {
    const ATTEMPTS: usize = 5;
    // What the destination held before this call began, so a partial-rename retry can establish
    // that the state it would settle is this call's own rather than another writer's value.
    let expected = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(SettingsFacadeError::Io {
                action: "read history replacement condition",
                path: path.to_owned(),
                source,
            });
        }
    };
    let mut last_error = None;
    for attempt in 0..ATTEMPTS {
        match atomic_write(path, bytes, None) {
            Ok(()) => return Ok(()),
            Err(error) => {
                // The destination already holding exactly our bytes is success: this history
                // entry is written for its content, so a concurrent writer that produced the
                // same content has already achieved the goal.
                if fs::read(path).is_ok_and(|current| current == bytes) {
                    return Ok(());
                }
                // A destination that holds **different** bytes is another writer's legitimate
                // result. The identity guard refused to replace it, and that refusal stands:
                // retrying here is exactly how a newer save would be overwritten by an older
                // one. Only a partial-rename state that provably left nothing else behind is
                // safe to settle.
                if !retry_settles_own_partial_state(&error, path, expected.as_deref()) {
                    return Err(error);
                }
                last_error = Some(error);
                if attempt + 1 < ATTEMPTS {
                    // A short, growing backoff, so a partial rename has time to settle
                    // without spinning.
                    std::thread::sleep(std::time::Duration::from_millis(10 << attempt));
                }
            }
        }
    }
    Err(last_error.expect("a failed history write records its error"))
}

/// Whether a failed history write may be retried without risking another writer's value.
///
/// The only retryable case is the native **partial rename** (`1176`/`1177`), whose defining
/// property is that the destination is left unchanged: the replacement name may or may not still
/// exist, but nothing new was published. When that holds, another attempt can only settle this
/// call's own residue.
///
/// Every other failure — and any failure whose destination now holds a *different* complete value —
/// is final. Retrying those is what would let an older write take the place of a newer one that a
/// concurrent facade had already saved, which the atomic-replace identity guard exists to prevent.
///
/// # What is evidence, and what is not
///
/// The previous form treated **any** `fs::read` error as "the
/// destination is missing", and an **empty** destination as this call's own residue. Neither has
/// any attribution behind it:
///
/// * a read that failed is "I could not ask", not "nothing is there" — a permission error, a
///   sharing violation or a transient I/O fault all look identical to a missing file from the
///   caller's side, and treating them as absence is exactly the "could not read therefore empty"
///   mistake this project refuses elsewhere;
/// * an empty file is a **value** some writer may have produced. Claiming it as this call's residue
///   and overwriting it is the same loss the identity guard exists to prevent, at a smaller size.
///
/// The retry is therefore allowed only when the destination is **provably** not holding another
/// writer's value, and the caller's own attempt is the only explanation left. That is established by
/// the caller's own expectation rather than by guessing: `expected` is the content this call
/// intended to replace, so a destination that still holds it — or that is absent because a read
/// succeeded and reported absence — is this call's own state.
fn retry_settles_own_partial_state(
    error: &SettingsFacadeError,
    path: &Path,
    expected: Option<&[u8]>,
) -> bool {
    let is_partial_rename = matches!(
        error,
        SettingsFacadeError::AtomicReplace(AtomicReplaceError::Windows { source, .. })
            if source.raw_os_error() == Some(1176) || source.raw_os_error() == Some(1177)
    );
    if !is_partial_rename {
        return false;
    }
    match fs::read(path) {
        // A read that succeeded and reported absence is real evidence: nothing is there to lose.
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => expected.is_none(),
        // Any other read error is "could not ask", which is NOT evidence of absence. Retrying could
        // overwrite a value this call never managed to inspect, so it is refused.
        Err(_) => false,
        // The destination still holds the value this call set out to replace, so the partial state
        // is this call's own and settling it loses nothing.
        Ok(current) => expected.is_some_and(|expected| current == expected),
    }
}

/// Preserves a domain's fault original before a recovery overwrites it.
///
/// A measurement showed that after hot-corrupting a file, a real
/// reload refusing, and then a reset per the current plan, the **original
/// non-secret content could not be found anywhere** in the data tree. The only copy
/// of the damaged input was the live file, and the recovery overwrote it — so the
/// evidence a user or a support engineer needs to understand what went wrong was
/// destroyed by the act of repairing it.
///
/// The original is kept in the domain's `invalid/` directory, which already exists
/// for startup recovery. Two rules are preserved from that path:
///
/// * a **secret-bearing** original is not copied into ordinary history — it stays
///   within the existing confidentiality boundary rather than being duplicated into
///   a plain-text file;
/// * a failed preservation is reported, not ignored, because the caller must be
///   able to decide not to overwrite the only copy.
///
/// Returns whether an original was actually preserved. A missing live file is not
/// an error: there is nothing to lose.
fn backup_invalid(
    history_dir: &Path,
    module: ConfigModule,
    bytes: &[u8],
) -> Result<(), SettingsFacadeError> {
    let invalid_dir = history_dir.join("invalid");
    fs::create_dir_all(&invalid_dir).map_err(|source| SettingsFacadeError::Io {
        action: "create invalid config backup directory",
        path: invalid_dir.clone(),
        source,
    })?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    atomic_write(&invalid_dir.join(format!("invalid-{stamp}.{}", module.extension())), bytes, None)
}

fn prune_history(history_dir: &Path) -> Result<(), SettingsFacadeError> {
    let now = SystemTime::now();
    let entries = fs::read_dir(history_dir).map_err(|source| SettingsFacadeError::Io {
        action: "read config history directory",
        path: history_dir.to_path_buf(),
        source,
    })?;
    let mut versions = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| SettingsFacadeError::Io {
            action: "read config history entry",
            path: history_dir.to_path_buf(),
            source,
        })?;
        if !entry.file_name().to_string_lossy().starts_with("version-") {
            continue;
        }
        let modified =
            entry.metadata().and_then(|metadata| metadata.modified()).unwrap_or(UNIX_EPOCH);
        if now.duration_since(modified).is_ok_and(|age| age > HISTORY_MAX_AGE) {
            let _ = fs::remove_file(entry.path());
        } else {
            versions.push((modified, entry.path()));
        }
    }
    versions.sort_by_key(|right| std::cmp::Reverse(right.0));
    for (_, path) in versions.into_iter().skip(HISTORY_LIMIT) {
        fs::remove_file(&path).map_err(|source| SettingsFacadeError::Io {
            action: "prune config history",
            path,
            source,
        })?;
    }
    Ok(())
}

fn cleanup_orphan_temps(config_root: &Path) -> Result<(), SettingsFacadeError> {
    for directory in [config_root.to_path_buf(), config_root.join("settings")] {
        let entries = fs::read_dir(&directory).map_err(|source| SettingsFacadeError::Io {
            action: "scan config temporary files",
            path: directory.clone(),
            source,
        })?;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".aura-") && name.ends_with(".tmp") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_read_detects_growth_after_metadata_without_returning_partial_bytes() {
        let directory = tempfile::tempdir().expect("temp root");
        let path = directory.path().join("models.json");
        fs::write(&path, vec![b' '; SETTINGS_FILE_MAX_BYTES as usize])
            .expect("write exact-limit file");

        let result = read_config_file_after_metadata(ConfigModule::Models, &path, || {
            let mut writer = OpenOptions::new().append(true).open(&path).expect("open append");
            writer.write_all(b"x").expect("grow config after metadata");
            writer.flush().expect("flush appended byte");
        });

        assert!(matches!(
            result,
            Err(SettingsFacadeError::FileTooLarge {
                module: ConfigModule::Models,
                limit_bytes: SETTINGS_FILE_MAX_BYTES,
            })
        ));
    }
}
