//! A namespace's locations from its records.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use aura_runtime_contract::{ActiveInstallState, BundleNamespace, PRODUCTION_BUNDLE_NAMESPACE};
use aura_windows_platform::DriveKind;
use serde::Serialize;

use crate::{
    Roots,
    record::{LocationRecord, RECORD_FILE, check_marker, read_record, store_record},
};

/// The Electron profile of a namespace that has none at Electron's default place.
pub(crate) const UI_PROFILE: &str = "ui-profile";

/// Where one installation keeps what it keeps.
///
/// Without a location record every path is where it has always been. With one, the data, Core's
/// log, the Electron profile and (for the released product) global memory are in the moved
/// folder; the settings, the Chrome files and the record itself stay in the default places,
/// because Windows' conventions and Chrome's remembered paths keep them there.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Locations {
    pub namespace_hash: String,
    /// The namespace's folder at the default place. It holds the record and the Chrome files and
    /// never moves; without a record it is also the data folder.
    pub fixed_root: PathBuf,
    pub data_root: PathBuf,
    pub settings_root: PathBuf,
    pub core_log: PathBuf,
    pub ui_profile: PathBuf,
    /// Chrome's manifest for the relay and the extension folder Chrome loads.
    pub chrome_root: PathBuf,
    pub record: PathBuf,
    /// The folder the data was moved to, which holds the marker; `None` in the default place.
    pub location: Option<PathBuf>,
    /// Copies of the data a move left that are not removed yet.
    pub pending_removal: Vec<PathBuf>,
    /// Aura's folders of every namespace in the default places.
    pub aura_roots: Vec<PathBuf>,
    /// Filled by [`Locations::with_global_memory`]; only Core needs it.
    pub global_memory: Option<PathBuf>,
}

/// What the installation folder says about the data's location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallCopy {
    /// No installation folder: a development run, a test, a tool.
    NotInstalled,
    /// `active.json`'s copy; `None` when it records no move.
    Installed(Option<PathBuf>),
}

impl InstallCopy {
    #[must_use]
    pub fn of(state: &ActiveInstallState) -> Self {
        Self::Installed(state.data_location.as_ref().map(PathBuf::from))
    }

    /// For a process started with `manifest`: installed when the manifest is in
    /// `<root>\versions\<version>\` and `<root>\active.json` is there, the layout the launcher
    /// and the installer write.
    pub fn for_manifest(manifest: &Path) -> Result<Self, LocationError> {
        Ok(installation(manifest)?.map_or(Self::NotInstalled, |(_, state)| Self::of(&state)))
    }
}

/// The installation a process started with `manifest` belongs to: its folder and `active.json`;
/// `None` for a development run, a test or a tool.
pub fn installation(
    manifest: &Path,
) -> Result<Option<(PathBuf, ActiveInstallState)>, LocationError> {
    let Some(versions) = manifest.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    if !versions.file_name().is_some_and(|name| name.eq_ignore_ascii_case("versions")) {
        return Ok(None);
    }
    let Some(root) = versions.parent() else {
        return Ok(None);
    };
    let active = root.join("active.json");
    let unreadable =
        |reason: String| LocationError::RecordUnreadable { path: active.clone(), reason };
    let bytes = match fs::read(&active) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(unreadable(error.to_string())),
    };
    let state: ActiveInstallState =
        serde_json::from_slice(&bytes).map_err(|error| unreadable(error.to_string()))?;
    Ok(Some((root.to_path_buf(), state)))
}

/// Why a namespace's locations cannot be given. Every kind means the data is not found: nothing
/// is created in its place.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LocationError {
    #[error("{0} is not set")]
    Environment(&'static str),
    #[error("a Windows known folder is unavailable: {0}")]
    KnownFolder(String),
    #[error("the location record {} cannot be used: {reason}", .path.display())]
    RecordUnreadable { path: PathBuf, reason: String },
    #[error(
        "the location record says {} but the installation's copy says {}",
        shown(.record),
        shown(.install_copy)
    )]
    RecordsDisagree { record: Option<PathBuf>, install_copy: Option<PathBuf> },
    #[error("the data folder {} is not there: {reason}", .location.display())]
    LocationMissing { location: PathBuf, reason: String },
    #[error("{} does not hold this installation's data: {reason}", .location.display())]
    MarkerMismatch { location: PathBuf, reason: String },
    #[error("{} is not on a fixed local disk ({kind:?})", .location.display())]
    NotFixedDisk { location: PathBuf, kind: DriveKind },
    #[error("the released product's data location cannot be found: {0}")]
    Production(Box<LocationError>),
}

fn shown(path: &Option<PathBuf>) -> String {
    path.as_ref().map_or_else(|| "nothing".to_owned(), |path| path.display().to_string())
}

impl LocationError {
    /// The data folder that is not there, for the desktop to name; none for other kinds.
    #[must_use]
    pub fn missing_location(&self) -> Option<&Path> {
        match self {
            Self::LocationMissing { location, .. } => Some(location),
            Self::Production(inner) => inner.missing_location(),
            _ => None,
        }
    }

    /// A stable name for the kind, for logs and for the desktop.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Environment(_) | Self::KnownFolder(_) => "environment",
            Self::RecordUnreadable { .. } => "record_unreadable",
            Self::RecordsDisagree { .. } => "records_disagree",
            Self::LocationMissing { .. } => "location_missing",
            Self::MarkerMismatch { .. } => "marker_mismatch",
            Self::NotFixedDisk { .. } => "not_fixed_disk",
            Self::Production(inner) => inner.code(),
        }
    }
}

fn is_production(namespace: &BundleNamespace) -> bool {
    namespace.as_str() == PRODUCTION_BUNDLE_NAMESPACE
}

impl Locations {
    /// Reads `namespace`'s record and checks the folder it names. Only reads: no folder is
    /// created and no file written.
    pub fn resolve(
        roots: &Roots,
        namespace: &BundleNamespace,
        install: &InstallCopy,
    ) -> Result<Self, LocationError> {
        let record_path = roots
            .local_aura()
            .join("namespaces")
            .join(namespace.object_name_hash())
            .join(RECORD_FILE);
        let record = read_record(&record_path, namespace)?;
        let copy = match install {
            InstallCopy::NotInstalled => None,
            InstallCopy::Installed(copy) => Some(copy.as_deref()),
        };
        let disagree =
            |record: Option<&Path>, copy: Option<&Path>| LocationError::RecordsDisagree {
                record: record.map(Path::to_path_buf),
                install_copy: copy.map(Path::to_path_buf),
            };
        let (place, pending) = match record {
            None => match copy {
                None | Some(None) => (None, Vec::new()),
                Some(copy) => return Err(disagree(None, copy)),
            },
            Some(record) => {
                match (record.location.as_deref(), copy) {
                    (None, Some(Some(copy))) => return Err(disagree(None, Some(copy))),
                    (Some(location), Some(None)) => return Err(disagree(Some(location), None)),
                    (Some(location), Some(Some(copy))) if !same_place(location, copy) => {
                        return Err(disagree(Some(location), Some(copy)));
                    }
                    (Some(location), _) => verify(location, namespace)?,
                    (None, _) => {}
                }
                (record.location, record.pending_removal)
            }
        };
        let mut locations = Self::for_place(roots, namespace, place.as_deref());
        locations.pending_removal = pending;
        Ok(locations)
    }

    /// Where everything would be with the data in `place` (`None`: the default places), whether
    /// or not it is there: what a move copies to. Only works the paths out.
    #[must_use]
    pub fn for_place(roots: &Roots, namespace: &BundleNamespace, place: Option<&Path>) -> Self {
        let hash = namespace.object_name_hash();
        let fixed_root = roots.local_aura().join("namespaces").join(&hash);
        let (data_root, core_log, ui_profile) = match place {
            None => (
                fixed_root.clone(),
                roots.local_aura().join("logs").join("core.jsonl"),
                if is_production(namespace) {
                    roots.roaming.join("@aura").join("desktop")
                } else {
                    fixed_root.join(UI_PROFILE)
                },
            ),
            Some(location) => (
                location.join("data"),
                location.join("logs").join("core.jsonl"),
                location.join(UI_PROFILE),
            ),
        };
        Self {
            settings_root: roots.roaming_aura().join("namespaces").join(&hash),
            namespace_hash: hash,
            data_root,
            core_log,
            ui_profile,
            chrome_root: fixed_root.join("chrome"),
            record: fixed_root.join(RECORD_FILE),
            location: place.map(Path::to_path_buf),
            pending_removal: Vec::new(),
            aura_roots: vec![roots.roaming_aura(), roots.local_aura()],
            global_memory: None,
            fixed_root,
        }
    }

    /// Every location inside two given folders: the settings in one, everything else in the
    /// other. For tests and tools that compose a Core over folders of their own; nothing in it
    /// points at the user's real profile.
    #[must_use]
    pub fn within(settings_root: PathBuf, data_root: PathBuf) -> Self {
        Self {
            namespace_hash: String::new(),
            fixed_root: data_root.clone(),
            settings_root,
            core_log: data_root.join("logs").join("core.jsonl"),
            ui_profile: data_root.join(UI_PROFILE),
            chrome_root: data_root.join("chrome"),
            record: data_root.join(RECORD_FILE),
            location: None,
            pending_removal: Vec::new(),
            aura_roots: Vec::new(),
            global_memory: None,
            data_root,
        }
    }

    /// Adds where global memory is. It is the released product's: in its moved folder when its
    /// data was moved, otherwise `%USERPROFILE%\.aura\memories`. Every other namespace uses the
    /// same one, found through the known local application data folder, so a development run
    /// with its own `LOCALAPPDATA` still follows a move of the released product's data.
    pub fn with_global_memory(
        mut self,
        roots: &Roots,
        namespace: &BundleNamespace,
    ) -> Result<Self, LocationError> {
        let location = if is_production(namespace) {
            self.location.clone()
        } else {
            let production: BundleNamespace =
                PRODUCTION_BUNDLE_NAMESPACE.parse().expect("the production namespace is valid");
            let known = Roots { local: roots.known_local.clone(), ..roots.clone() };
            Self::resolve(&known, &production, &InstallCopy::NotInstalled)
                .map_err(|error| LocationError::Production(Box::new(error)))?
                .location
        };
        self.global_memory = Some(location.map_or_else(
            || roots.profile.join(".aura").join("memories"),
            |location| location.join("memories"),
        ));
        Ok(self)
    }

    /// The folders a model is asked about every time it reaches into one: the data, the folder
    /// of Core's log, the Electron profile, the settings, copies a move left and Aura's folders
    /// of every namespace. Global memory is not among them; it stays an ordinary folder.
    #[must_use]
    pub fn protected_folders(&self) -> Vec<PathBuf> {
        let mut folders = vec![self.data_root.clone(), self.settings_root.clone()];
        folders.extend(self.core_log.parent().map(Path::to_path_buf));
        folders.push(self.ui_profile.clone());
        folders.extend(self.pending_removal.iter().cloned());
        folders.extend(self.aura_roots.iter().cloned());
        folders
    }

    /// The files that say where the data is, which only Aura writes.
    #[must_use]
    pub fn control_files(&self) -> Vec<PathBuf> {
        let mut files = vec![self.record.clone()];
        files.extend(self.location.as_ref().map(|location| location.join(crate::MARKER_FILE)));
        files
    }
}

/// Whether `location` can hold this namespace's data: a folder on a fixed local disk with the
/// namespace's marker.
pub fn verify_location(location: &Path, namespace: &BundleNamespace) -> Result<(), LocationError> {
    verify(location, namespace)
}

/// Points the location record at `location`, the folder this installation's data is in now: for
/// a data folder moved by hand or a drive that got another letter. Only a folder with this
/// installation's marker is taken, and not a copy the record lists for removal, which is the data
/// as it was before a move. The copies the record lists stay listed.
pub fn point_at(
    roots: &Roots,
    namespace: &BundleNamespace,
    location: &Path,
) -> Result<(), LocationError> {
    verify(location, namespace)?;
    let record = Locations::for_place(roots, namespace, None).record;
    // A record that cannot be read is the one being replaced.
    let pending = read_record(&record, namespace)
        .ok()
        .flatten()
        .map(|kept| kept.pending_removal)
        .unwrap_or_default();
    if pending.iter().any(|copy| same_place(copy, location)) {
        return Err(LocationError::MarkerMismatch {
            location: location.to_path_buf(),
            reason: "left behind by a move, to be removed".to_owned(),
        });
    }
    let pointed = LocationRecord::with_removals(namespace.clone(), Some(location.into()), pending);
    store_record(&record, &pointed).map_err(|error| LocationError::RecordUnreadable {
        path: record.clone(),
        reason: error.to_string(),
    })
}

fn verify(location: &Path, namespace: &BundleNamespace) -> Result<(), LocationError> {
    let missing = |reason: String| LocationError::LocationMissing {
        location: location.to_path_buf(),
        reason,
    };
    match fs::metadata(location) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(missing("not a folder".to_owned())),
        Err(error) => return Err(missing(error.to_string())),
    }
    match aura_windows_platform::drive_kind(location) {
        Ok(DriveKind::Fixed) => {}
        Ok(kind) => {
            return Err(LocationError::NotFixedDisk { location: location.to_path_buf(), kind });
        }
        Err(error) => return Err(missing(error.to_string())),
    }
    check_marker(location, namespace).map_err(|reason| LocationError::MarkerMismatch {
        location: location.to_path_buf(),
        reason,
    })
}

/// Whether two spellings name the same folder: Windows paths ignore case and a trailing separator.
#[must_use]
pub fn same_place(left: &Path, right: &Path) -> bool {
    let plain = |path: &Path| {
        path.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase()
    };
    plain(left) == plain(right)
}
