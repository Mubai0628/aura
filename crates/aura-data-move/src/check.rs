//! Whether a folder the user chose can take the data, and if not, every reason why. Only reads:
//! the sizes come from walking what would be copied.
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use aura_data_location::{Locations, Roots};
use aura_runtime_contract::BundleNamespace;
use aura_windows_platform::DriveKind;

use crate::plan::{MovePlan, plan, units, within};

/// The paths Windows takes without long-path support, less the terminating NUL.
const MAX_PATH_CHARS: usize = 259;
/// The deepest path Aura writes below a data folder: a memory vault's maintenance file.
const DEEPEST_IN_DATA: usize = 174;
/// The longest location folder a move takes, in characters: its `data` folder's deepest file
/// still fits in `MAX_PATH_CHARS`, so no program that reads the data needs long-path support.
pub const MAX_LOCATION_CHARS: usize = MAX_PATH_CHARS - r"\data\".len() - DEEPEST_IN_DATA;
const GIB: u64 = 1 << 30;

/// What a check needs beyond the folder chosen.
pub struct Context<'a> {
    pub roots: &'a Roots,
    pub namespace: &'a BundleNamespace,
    /// The locations now, with global memory.
    pub current: &'a Locations,
    /// The installation's folder; `None` in a development run.
    pub install_root: Option<&'a Path>,
    pub workspaces: &'a [PathBuf],
    /// A new version is on trial: nothing moves until it is confirmed or put back.
    pub on_trial: bool,
    pub production: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Choice {
    Folder(PathBuf),
    DefaultPlace,
}

#[derive(Clone, Debug)]
pub struct Accepted {
    pub destination: Option<PathBuf>,
    pub plan: MovePlan,
    pub needed: u64,
    pub free: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    OnTrial,
    AlreadyThere,
    NotAbsolute,
    NotFixedDisk(DriveKind),
    SyncFolder(String),
    IsAFile(PathBuf),
    NotEmpty(PathBuf),
    InInstallation,
    HoldsInstallation,
    InWorkspace(PathBuf),
    HoldsWorkspace(PathBuf),
    InData,
    HoldsData,
    /// `most`: the longest the location can be, for the data as deep as it is.
    TooLong {
        length: usize,
        most: usize,
    },
    NoRoom {
        needed: u64,
        free: u64,
    },
    DefaultTaken(PathBuf),
    Unreadable(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mib = |bytes: u64| format!("{} MB", bytes.div_ceil(1 << 20));
        match self {
            Self::OnTrial => write!(formatter, "更新还在试用，确认能用后再搬"),
            Self::AlreadyThere => write!(formatter, "数据已经在这里"),
            Self::NotAbsolute => write!(formatter, "不是完整的路径"),
            Self::NotFixedDisk(kind) => {
                let kind = match kind {
                    DriveKind::Removable => "U 盘或移动硬盘",
                    DriveKind::Remote => "网络位置",
                    DriveKind::Optical => "光盘",
                    DriveKind::RamDisk => "内存盘",
                    DriveKind::Fixed | DriveKind::Unknown => "未知的磁盘",
                };
                write!(formatter, "这是{kind}，只能选本机的固定磁盘")
            }
            Self::SyncFolder(name) if name.is_empty() => write!(formatter, "在同步文件夹里"),
            Self::SyncFolder(name) => write!(formatter, "在 {name} 的同步文件夹里"),
            Self::IsAFile(path) => write!(formatter, "{} 是文件", path.display()),
            Self::NotEmpty(path) => write!(formatter, "{} 不是空文件夹", path.display()),
            Self::InInstallation => write!(formatter, "在 Aura 的安装文件夹里"),
            Self::HoldsInstallation => write!(formatter, "里面有 Aura 的安装文件夹"),
            Self::InWorkspace(path) => write!(formatter, "在工作区 {} 里", path.display()),
            Self::HoldsWorkspace(path) => write!(formatter, "里面有工作区 {}", path.display()),
            Self::InData => write!(formatter, "在现在的数据文件夹里"),
            Self::HoldsData => write!(formatter, "里面有现在的数据"),
            Self::TooLong { length, most } => {
                write!(formatter, "路径有 {length} 个字符，最多 {most} 个")
            }
            Self::NoRoom { needed, free } => {
                write!(formatter, "空间不够：需要约 {}，这个磁盘只剩 {}", mib(*needed), mib(*free))
            }
            Self::DefaultTaken(path) => write!(formatter, "默认位置还有旧数据：{}", path.display()),
            Self::Unreadable(reason) => write!(formatter, "无法检查：{reason}"),
        }
    }
}

/// Room a copy of `bytes` needs: a tenth more, and 1 GB besides.
#[must_use]
pub fn room_needed(bytes: u64) -> u64 {
    bytes + bytes / 10 + GIB
}

/// Checks `choice`; every refusal that applies is returned, not just the first.
pub fn check(context: &Context<'_>, choice: &Choice) -> Result<Accepted, Vec<Refusal>> {
    let mut refusals = Vec::new();
    if context.on_trial {
        refusals.push(Refusal::OnTrial);
    }
    let destination =
        match choice {
            Choice::DefaultPlace => {
                if context.current.location.is_none() {
                    return Err(vec![Refusal::AlreadyThere]);
                }
                None
            }
            Choice::Folder(chosen)
                if context.current.location.as_deref().is_some_and(|location| {
                    within(chosen, location) && within(location, chosen)
                }) =>
            {
                return Err(vec![Refusal::AlreadyThere]);
            }
            Choice::Folder(chosen) => match destination_in(chosen) {
                Ok(destination) => Some(destination),
                Err(refusal) => {
                    refusals.push(refusal);
                    return Err(refusals);
                }
            },
        };
    let target = Locations::for_place(context.roots, context.namespace, destination.as_deref())
        .with_global_memory(context.roots, context.namespace)
        .map_err(|error| vec![Refusal::Unreadable(error.to_string())])?;
    match &destination {
        Some(destination) => folder_rules(context, destination, &mut refusals),
        None => default_rules(&target, context.production, &mut refusals),
    }
    let plan = match plan(context.current, &target, context.production) {
        Ok(plan) => plan,
        Err(error) => {
            refusals.push(Refusal::Unreadable(error.to_string()));
            return Err(refusals);
        }
    };
    if let Some(location) = &destination {
        // Data deeper than Aura writes, put there by hand, lowers the limit further.
        let most = MAX_LOCATION_CHARS.min(MAX_PATH_CHARS.saturating_sub(1 + plan.longest));
        let length = units(location);
        if length > most {
            refusals.push(Refusal::TooLong { length, most });
        }
    }
    let needed = room_needed(plan.bytes);
    let volume = destination.as_deref().unwrap_or(&target.fixed_root);
    let free = existing(volume)
        .and_then(|folder| aura_windows_platform::free_disk_space(folder).ok())
        .unwrap_or(0);
    if free < needed {
        refusals.push(Refusal::NoRoom { needed, free });
    }
    if refusals.is_empty() {
        Ok(Accepted { destination, plan, needed, free })
    } else {
        Err(refusals)
    }
}

/// The location a chosen folder gives: the folder itself when it is new or empty, otherwise an
/// `Aura` folder inside it, which must be new or empty in turn.
fn destination_in(chosen: &Path) -> Result<PathBuf, Refusal> {
    if !chosen.is_absolute() {
        return Err(Refusal::NotAbsolute);
    }
    let chosen =
        std::path::absolute(chosen).map_err(|error| Refusal::Unreadable(error.to_string()))?;
    match emptiness(&chosen)? {
        Emptiness::Missing | Emptiness::Empty => Ok(chosen),
        Emptiness::HasEntries => {
            let inside = chosen.join("Aura");
            match emptiness(&inside)? {
                Emptiness::Missing | Emptiness::Empty => Ok(inside),
                Emptiness::HasEntries => Err(Refusal::NotEmpty(inside)),
            }
        }
    }
}

enum Emptiness {
    Missing,
    Empty,
    HasEntries,
}

fn emptiness(path: &Path) -> Result<Emptiness, Refusal> {
    match fs::metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Emptiness::Missing),
        Err(error) => Err(Refusal::Unreadable(error.to_string())),
        Ok(metadata) if !metadata.is_dir() => Err(Refusal::IsAFile(path.to_path_buf())),
        Ok(_) => {
            let mut entries =
                fs::read_dir(path).map_err(|error| Refusal::Unreadable(error.to_string()))?;
            Ok(if entries.next().is_some() { Emptiness::HasEntries } else { Emptiness::Empty })
        }
    }
}

fn folder_rules(context: &Context<'_>, destination: &Path, refusals: &mut Vec<Refusal>) {
    let current = context.current;
    match existing(destination).map(aura_windows_platform::drive_kind) {
        Some(Ok(DriveKind::Fixed)) => {}
        Some(Ok(kind)) => refusals.push(Refusal::NotFixedDisk(kind)),
        Some(Err(error)) => refusals.push(Refusal::Unreadable(error.to_string())),
        None => refusals.push(Refusal::Unreadable("no folder above it is there".to_owned())),
    }
    if let Some(name) = aura_windows_platform::sync_folder_of(destination) {
        refusals.push(Refusal::SyncFolder(name));
    }
    if let Some(install) = context.install_root {
        if within(destination, install) {
            refusals.push(Refusal::InInstallation);
        } else if within(install, destination) {
            refusals.push(Refusal::HoldsInstallation);
        }
    }
    for workspace in context.workspaces {
        if within(destination, workspace) {
            refusals.push(Refusal::InWorkspace(workspace.clone()));
        } else if within(workspace, destination) {
            refusals.push(Refusal::HoldsWorkspace(workspace.clone()));
        }
    }
    let mut in_data = false;
    let mut holds_data = false;
    for place in data_places(current) {
        in_data |= within(destination, &place);
        holds_data |= within(&place, destination);
    }
    if in_data {
        refusals.push(Refusal::InData);
    } else if holds_data {
        refusals.push(Refusal::HoldsData);
    }
}

/// Every folder or file the data is in now, and Aura's own folders, which uninstalling removes.
fn data_places(current: &Locations) -> Vec<PathBuf> {
    let mut places = vec![current.fixed_root.clone(), current.data_root.clone()];
    places.extend(current.aura_roots.iter().cloned());
    places.extend(current.location.clone());
    places.push(current.ui_profile.clone());
    places.push(current.settings_root.clone());
    places.extend(current.global_memory.clone());
    places.extend(current.pending_removal.iter().cloned());
    places
}

/// Before the copy starts, what can have changed since the check: the destination must still be
/// new or empty, on a fixed disk, with room.
pub(crate) fn recheck(target: &Locations, plan: &MovePlan, production: bool) -> Vec<Refusal> {
    let mut refusals = Vec::new();
    match &plan.destination {
        Some(location) => {
            match emptiness(location) {
                Ok(Emptiness::Missing | Emptiness::Empty) => {}
                Ok(Emptiness::HasEntries) => refusals.push(Refusal::NotEmpty(location.clone())),
                Err(refusal) => refusals.push(refusal),
            }
            match existing(location).map(aura_windows_platform::drive_kind) {
                Some(Ok(DriveKind::Fixed)) => {}
                Some(Ok(kind)) => refusals.push(Refusal::NotFixedDisk(kind)),
                Some(Err(error)) => refusals.push(Refusal::Unreadable(error.to_string())),
                None => {
                    refusals.push(Refusal::Unreadable("no folder above it is there".to_owned()))
                }
            }
        }
        None => default_rules(target, production, &mut refusals),
    }
    let needed = room_needed(plan.bytes);
    let volume = plan.destination.as_deref().unwrap_or(&target.fixed_root);
    let free = existing(volume)
        .and_then(|folder| aura_windows_platform::free_disk_space(folder).ok())
        .unwrap_or(0);
    if free < needed {
        refusals.push(Refusal::NoRoom { needed, free });
    }
    refusals
}

/// Moving back: the default places must not hold data from before, or it would mix with the copy.
fn default_rules(target: &Locations, production: bool, refusals: &mut Vec<Refusal>) {
    let fixed = &target.fixed_root;
    if let Ok(entries) = fs::read_dir(fixed) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let kept = ["chrome", aura_data_location::RECORD_FILE, crate::journal::JOURNAL_FILE]
                .iter()
                .any(|kept| kept.eq_ignore_ascii_case(&name))
                || name.ends_with(".writing");
            if !kept {
                refusals.push(Refusal::DefaultTaken(entry.path()));
            }
        }
    }
    let mut places = vec![target.ui_profile.clone()];
    if production {
        places.extend(target.global_memory.clone());
    }
    for place in places {
        if !place.starts_with(fixed)
            && fs::read_dir(&place).is_ok_and(|mut entries| entries.next().is_some())
        {
            refusals.push(Refusal::DefaultTaken(place));
        }
    }
}

/// The nearest folder at or above `path` that is there.
fn existing(path: &Path) -> Option<&Path> {
    path.ancestors().find(|folder| folder.is_dir())
}
