//! Whether a folder can take the installation, and if not, every reason why. Only reads.
use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

use aura_data_location::Locations;
use aura_data_move::within;
use aura_windows_platform::DriveKind;

/// The paths Windows takes without long-path support, less the terminating NUL.
const MAX_PATH_CHARS: usize = 259;
const GIB: u64 = 1 << 30;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    AlreadyThere,
    NotAbsolute,
    NotFixedDisk(DriveKind),
    SyncFolder(String),
    IsAFile(PathBuf),
    NotEmpty(PathBuf),
    InData,
    HoldsData,
    InInstallation,
    HoldsInstallation,
    /// `most`: the longest the folder can be for the deepest file the installation has.
    TooLong {
        length: usize,
        most: usize,
    },
    NoRoom {
        needed: u64,
        free: u64,
    },
    Unreadable(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mib = |bytes: u64| format!("{} MB", bytes.div_ceil(1 << 20));
        match self {
            Self::AlreadyThere => write!(formatter, "Aura 已经装在这里"),
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
            Self::InData => write!(formatter, "在 Aura 的数据文件夹里"),
            Self::HoldsData => write!(formatter, "里面有 Aura 的数据"),
            Self::InInstallation => write!(formatter, "在现在的安装文件夹里"),
            Self::HoldsInstallation => write!(formatter, "里面有现在的安装文件夹"),
            Self::TooLong { length, most } => {
                write!(formatter, "路径有 {length} 个字符，最多 {most} 个")
            }
            Self::NoRoom { needed, free } => {
                write!(formatter, "空间不够：需要约 {}，这个磁盘只剩 {}", mib(*needed), mib(*free))
            }
            Self::Unreadable(reason) => write!(formatter, "无法检查：{reason}"),
        }
    }
}

/// Every refusal, joined for one line of a page or a quiet run's answer.
#[must_use]
pub fn joined(refusals: &[Refusal]) -> String {
    refusals.iter().map(ToString::to_string).collect::<Vec<_>>().join("；")
}

/// The installation folder a folder picked in the window gives: the folder itself when it is
/// called `Aura`, otherwise an `Aura` folder inside it.
#[must_use]
pub fn root_in(picked: &Path) -> PathBuf {
    if picked.file_name().is_some_and(|name| name.eq_ignore_ascii_case("Aura")) {
        picked.to_path_buf()
    } else {
        picked.join("Aura")
    }
}

/// What the installation brings: its bytes and the longest path inside it, in UTF-16 units.
#[derive(Clone, Copy, Debug)]
pub struct Size {
    pub bytes: u64,
    pub longest: usize,
}

/// The size of the installation in `root` as it is now.
#[must_use]
pub fn size_of(root: &Path) -> Size {
    let mut size = Size { bytes: 0, longest: 0 };
    walk(root, root, &mut size);
    size
}

fn walk(root: &Path, folder: &Path, size: &mut Size) {
    for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        if let Ok(inside) = path.strip_prefix(root) {
            size.longest = size.longest.max(units(inside));
        }
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(root, &path, size),
            Ok(_) => size.bytes += entry.metadata().map(|metadata| metadata.len()).unwrap_or(0),
            Err(_) => {}
        }
    }
}

fn units(path: &Path) -> usize {
    path.to_string_lossy().encode_utf16().count()
}

/// Checks `root` for an installation of `size`; `locations` are the data's places now, which the
/// installation may neither be in nor hold. `current` is the installation's folder now, if any.
pub fn check_root(
    root: &Path,
    size: Size,
    locations: &Locations,
    current: Option<&Path>,
) -> Result<PathBuf, Vec<Refusal>> {
    if !root.is_absolute() {
        return Err(vec![Refusal::NotAbsolute]);
    }
    let root = std::path::absolute(root).map_err(|error| vec![unreadable(&error)])?;
    if current.is_some_and(|current| within(&root, current) && within(current, &root)) {
        return Err(vec![Refusal::AlreadyThere]);
    }
    let mut refusals = Vec::new();
    match fs::metadata(&root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => refusals.push(unreadable(&error)),
        Ok(metadata) if !metadata.is_dir() => refusals.push(Refusal::IsAFile(root.clone())),
        Ok(_) => match fs::read_dir(&root).map(|mut entries| entries.next().is_some()) {
            Ok(true) => refusals.push(Refusal::NotEmpty(root.clone())),
            Ok(false) => {}
            Err(error) => refusals.push(unreadable(&error)),
        },
    }
    let existing = root.ancestors().find(|folder| folder.is_dir());
    match existing.map(aura_windows_platform::drive_kind) {
        Some(Ok(DriveKind::Fixed)) => {}
        Some(Ok(kind)) => refusals.push(Refusal::NotFixedDisk(kind)),
        Some(Err(error)) => refusals.push(unreadable(&error)),
        None => refusals.push(Refusal::Unreadable("上面的文件夹都不在".to_owned())),
    }
    if let Some(name) = aura_windows_platform::sync_folder_of(&root) {
        refusals.push(Refusal::SyncFolder(name));
    }
    if let Some(current) = current {
        if within(&root, current) {
            refusals.push(Refusal::InInstallation);
        } else if within(current, &root) {
            refusals.push(Refusal::HoldsInstallation);
        }
    }
    let places = data_places(locations);
    if places.iter().any(|place| within(&root, place)) {
        refusals.push(Refusal::InData);
    } else if places.iter().any(|place| within(place, &root)) {
        refusals.push(Refusal::HoldsData);
    }
    let most = MAX_PATH_CHARS.saturating_sub(1 + size.longest);
    let length = units(&root);
    if length > most {
        refusals.push(Refusal::TooLong { length, most });
    }
    let needed = size.bytes + size.bytes / 10 + GIB;
    let free = existing.and_then(|folder| aura_windows_platform::free_disk_space(folder).ok());
    match free {
        Some(free) if free < needed => refusals.push(Refusal::NoRoom { needed, free }),
        _ => {}
    }
    if refusals.is_empty() { Ok(root) } else { Err(refusals) }
}

fn unreadable(error: &io::Error) -> Refusal {
    Refusal::Unreadable(error.to_string())
}

/// The data's folders and Aura's own, which an installation folder stays out of.
fn data_places(locations: &Locations) -> Vec<PathBuf> {
    let mut places = vec![locations.fixed_root.clone(), locations.data_root.clone()];
    places.extend(locations.aura_roots.iter().cloned());
    places.extend(locations.location.clone());
    places.push(locations.ui_profile.clone());
    places.push(locations.settings_root.clone());
    places.extend(locations.global_memory.clone());
    places.extend(locations.pending_removal.iter().cloned());
    places
}
