//! What a move copies: the data, Core's log, the window profile without the caches Chromium
//! rebuilds and, for the released product, global memory; each from where it is to where it goes,
//! with its size. The settings, the credentials, the Chrome folder and the location record stay.
use std::{
    fs, io,
    os::windows::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};

use aura_data_location::{Locations, RECORD_FILE, UI_PROFILE_FOLDER};
use serde::{Deserialize, Serialize};

use crate::journal::JOURNAL_FILE;

/// Folder names Chromium rebuilds by itself, at any depth of the window profile.
const CACHES: [&str; 8] = [
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GraphiteDawnCache",
    "GrShaderCache",
    "ShaderCache",
];

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Data,
    CoreLog,
    WindowProfile,
    GlobalMemory,
}

/// One thing that moves.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub part: Part,
    pub from: PathBuf,
    pub to: PathBuf,
    /// Names directly under `from` that stay where they are.
    pub skip: Vec<String>,
    /// Chromium's caches, at any depth, stay behind and are rebuilt.
    pub skip_caches: bool,
    /// `from` is one file that is added to the end of the file already at `to`.
    pub append: bool,
    pub bytes: u64,
    pub files: u64,
}

/// Everything one move does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MovePlan {
    /// Where the data is now; `None`: the default places.
    pub from: Option<PathBuf>,
    /// Where it goes; `None`: back to the default places.
    pub destination: Option<PathBuf>,
    pub items: Vec<Item>,
    /// What stays behind to be removed once the new place has started.
    pub old_copies: Vec<PathBuf>,
    pub bytes: u64,
    /// The longest path below a destination folder that the copy writes, in characters.
    pub longest: usize,
}

/// The move from `current` (resolved, with global memory) to `target` (the same places worked
/// out for the destination). `production` says whether global memory moves with the data.
pub fn plan(current: &Locations, target: &Locations, production: bool) -> io::Result<MovePlan> {
    let mut items = Vec::new();
    let mut old_copies = Vec::new();
    let at_default = current.location.is_none();
    let mut skip = Vec::new();
    if at_default {
        // The folder that never moves holds the data at the default place, and the files that
        // stay with it.
        skip.extend(["chrome", RECORD_FILE, JOURNAL_FILE].map(str::to_owned));
        if current.ui_profile.starts_with(&current.data_root) {
            skip.push(UI_PROFILE_FOLDER.to_owned());
        }
    }
    items.push(Item {
        part: Part::Data,
        from: current.data_root.clone(),
        to: target.data_root.clone(),
        skip: skip.clone(),
        skip_caches: false,
        append: false,
        bytes: 0,
        files: 0,
    });
    if at_default && current.data_root.is_dir() {
        for entry in fs::read_dir(&current.data_root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !skip.iter().any(|skipped| skipped.eq_ignore_ascii_case(&name)) {
                old_copies.push(entry.path());
            }
        }
    }
    // The log at the default places is the user's, which every installation there writes: like
    // global memory, only the released product's moves with its data.
    if (production || !at_default) && current.core_log.is_file() {
        items.push(Item {
            part: Part::CoreLog,
            from: current.core_log.clone(),
            to: target.core_log.clone(),
            skip: Vec::new(),
            skip_caches: false,
            append: target.core_log.is_file(),
            bytes: 0,
            files: 0,
        });
        if at_default {
            old_copies.push(current.core_log.clone());
        }
    }
    if current.ui_profile.is_dir() {
        items.push(Item {
            part: Part::WindowProfile,
            from: current.ui_profile.clone(),
            to: target.ui_profile.clone(),
            skip: Vec::new(),
            skip_caches: true,
            append: false,
            bytes: 0,
            files: 0,
        });
        if at_default {
            old_copies.push(current.ui_profile.clone());
        }
    }
    if production
        && let (Some(from), Some(to)) = (&current.global_memory, &target.global_memory)
        && from.is_dir()
    {
        items.push(Item {
            part: Part::GlobalMemory,
            from: from.clone(),
            to: to.clone(),
            skip: Vec::new(),
            skip_caches: false,
            append: false,
            bytes: 0,
            files: 0,
        });
        if at_default {
            old_copies.push(from.clone());
        }
    }
    if let Some(location) = &current.location {
        old_copies.push(location.clone());
    }
    let mut longest = 0;
    let mut bytes = 0;
    for item in &mut items {
        let measured = measure(item)?;
        item.bytes = measured.bytes;
        item.files = measured.files;
        bytes += measured.bytes;
        let below = item_root_length(item, target);
        longest = longest.max(below + measured.longest);
    }
    Ok(MovePlan {
        from: current.location.clone(),
        destination: target.location.clone(),
        items,
        old_copies,
        bytes,
        longest,
    })
}

/// What a move from `current` copies, in bytes: the data, Core's log, the window profile without
/// its caches and, for the released product, global memory.
pub fn data_size(current: &Locations, production: bool) -> io::Result<u64> {
    Ok(plan(current, current, production)?.bytes)
}

/// What `folder` holds, in bytes, as far as it can be read; links are not followed.
#[must_use]
pub fn folder_size(folder: &Path) -> u64 {
    let mut bytes = 0;
    let mut pending = vec![folder.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
            let Ok(metadata) = entry.metadata() else { continue };
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                bytes += metadata.len();
            }
        }
    }
    bytes
}

/// How far below the destination folder an item's own folder is, in characters.
fn item_root_length(item: &Item, target: &Locations) -> usize {
    target
        .location
        .as_ref()
        .and_then(|location| item.to.strip_prefix(location).ok())
        .map_or(0, |below| units(below) + 1)
}

/// A path's length as Windows counts it, in UTF-16 units.
pub(crate) fn units(path: &Path) -> usize {
    path.as_os_str().encode_wide().count()
}

#[derive(Default)]
struct Measured {
    bytes: u64,
    files: u64,
    longest: usize,
}

fn measure(item: &Item) -> io::Result<Measured> {
    let mut measured = Measured::default();
    // Before the first start there is no data folder: nothing to copy.
    if item.part == Part::Data && !item.from.exists() {
        return Ok(measured);
    }
    if !item.from.is_dir() {
        measured.bytes = fs::metadata(&item.from)?.len();
        measured.files = 1;
        return Ok(measured);
    }
    let mut pending = vec![(item.from.clone(), true)];
    while let Some((folder, top)) = pending.pop() {
        for entry in fs::read_dir(&folder)? {
            let entry = entry?;
            if !copied(item, &entry.file_name().to_string_lossy(), top) {
                continue;
            }
            let metadata = entry.metadata()?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(io::Error::other(format!(
                    "{} 是链接，数据里不该有链接",
                    entry.path().display()
                )));
            }
            let below = entry.path().strip_prefix(&item.from).map_or(0, units);
            measured.longest = measured.longest.max(below);
            if metadata.is_dir() {
                pending.push((entry.path(), false));
            } else {
                measured.bytes += metadata.len();
                measured.files += 1;
            }
        }
    }
    Ok(measured)
}

/// Whether the entry `name` (directly under the item's folder when `top`) moves.
pub(crate) fn copied(item: &Item, name: &str, top: bool) -> bool {
    if top && item.skip.iter().any(|skipped| skipped.eq_ignore_ascii_case(name)) {
        return false;
    }
    !(item.skip_caches && CACHES.iter().any(|cache| cache.eq_ignore_ascii_case(name)))
}

/// Whether `path` is `folder` or inside it: Windows paths ignore case and the separator's kind.
#[must_use]
pub fn within(path: &Path, folder: &Path) -> bool {
    let plain = |path: &Path| {
        let text = path.to_string_lossy().replace('/', "\\").to_lowercase();
        PathBuf::from(text.trim_end_matches('\\'))
    };
    plain(path).starts_with(plain(folder))
}
