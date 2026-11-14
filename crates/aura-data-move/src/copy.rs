//! Copying an item and checking the copy. Each file is hashed as it is read, written to a new
//! file that is flushed to the disk before the next one starts, and read back once everything is
//! copied: a copy is trusted only when every file's size and hash match.
use std::{
    fs::{self, File, FileTimes, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use crate::{
    journal::Created,
    plan::{Item, copied},
};

const CHUNK: usize = 1 << 20;

/// One file the copy wrote: where, how much from where, and the hash of what it wrote.
#[derive(Clone, Debug)]
pub(crate) struct Written {
    pub path: PathBuf,
    pub offset: u64,
    pub bytes: u64,
    pub hash: blake3::Hash,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CopyError {
    #[error("已停止")]
    Cancelled,
    #[error("{}：{source}", .path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("{} 复制后核对不上", .0.display())]
    Mismatch(PathBuf),
}

fn at(path: &Path) -> impl FnOnce(io::Error) -> CopyError + '_ {
    move |source| CopyError::Io { path: path.to_path_buf(), source }
}

/// What the copy is told and tells as it goes.
pub(crate) struct Copying<'a> {
    /// Called before each thing the copy makes at the destination, so it is recorded first.
    pub made: &'a mut dyn FnMut(Created) -> io::Result<()>,
    /// Bytes copied so far, across all items.
    pub copied: &'a mut dyn FnMut(u64),
    pub cancelled: &'a dyn Fn() -> bool,
    /// Every folder and file the copy makes is kept out of Windows Search's index.
    pub not_indexed: bool,
}

/// Copies `item`, returning each file written.
pub(crate) fn copy_item(item: &Item, copying: &mut Copying<'_>) -> Result<Vec<Written>, CopyError> {
    let mut written = Vec::new();
    if !item.from.is_dir() {
        if let Some(parent) = item.to.parent() {
            make_folders(parent, copying)?;
        }
        written.push(copy_file(&item.from, &item.to, item.append, copying)?);
        return Ok(written);
    }
    make_folders(&item.to, copying)?;
    let mut pending = vec![(item.from.clone(), item.to.clone(), true)];
    while let Some((from, to, top)) = pending.pop() {
        let mut entries = fs::read_dir(&from)
            .map_err(at(&from))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(at(&from))?;
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name();
            if !copied(item, &name.to_string_lossy(), top) {
                continue;
            }
            let source = entry.path();
            let target = to.join(&name);
            let kind = entry.file_type().map_err(at(&source))?;
            if kind.is_dir() {
                (copying.made)(Created::Folder { path: target.clone() }).map_err(at(&target))?;
                fs::create_dir(&target).map_err(at(&target))?;
                index_off(&target, copying)?;
                pending.push((source, target, false));
            } else if kind.is_file() {
                written.push(copy_file(&source, &target, false, copying)?);
            } else {
                return Err(CopyError::Io {
                    path: source,
                    source: io::Error::other("是链接，数据里不该有链接"),
                });
            }
        }
    }
    Ok(written)
}

/// Makes `folder` and the folders above it that are not there, recording each before it is made.
fn make_folders(folder: &Path, copying: &mut Copying<'_>) -> Result<(), CopyError> {
    let missing: Vec<&Path> = folder.ancestors().take_while(|path| !path.exists()).collect();
    for path in missing.into_iter().rev() {
        (copying.made)(Created::Folder { path: path.to_path_buf() }).map_err(at(path))?;
        fs::create_dir(path).map_err(at(path))?;
        index_off(path, copying)?;
    }
    Ok(())
}

fn index_off(path: &Path, copying: &Copying<'_>) -> Result<(), CopyError> {
    if copying.not_indexed {
        aura_windows_platform::keep_out_of_index(path).map_err(at(path))?;
    }
    Ok(())
}

fn copy_file(
    from: &Path,
    to: &Path,
    append: bool,
    copying: &mut Copying<'_>,
) -> Result<Written, CopyError> {
    let mut source = File::open(from).map_err(at(from))?;
    let metadata = source.metadata().map_err(at(from))?;
    let (mut target, offset) = if append {
        let length = fs::metadata(to).map_err(at(to))?.len();
        (copying.made)(Created::Appended { path: to.to_path_buf(), length }).map_err(at(to))?;
        let mut file = OpenOptions::new().write(true).open(to).map_err(at(to))?;
        file.seek(SeekFrom::End(0)).map_err(at(to))?;
        (file, length)
    } else {
        (copying.made)(Created::File { path: to.to_path_buf() }).map_err(at(to))?;
        (OpenOptions::new().write(true).create_new(true).open(to).map_err(at(to))?, 0)
    };
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0_u8; CHUNK];
    let mut bytes = 0_u64;
    loop {
        if (copying.cancelled)() {
            return Err(CopyError::Cancelled);
        }
        let read = source.read(&mut buffer).map_err(at(from))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        target.write_all(&buffer[..read]).map_err(at(to))?;
        bytes += read as u64;
        (copying.copied)(read as u64);
    }
    if !append && let Ok(modified) = metadata.modified() {
        let _ = target.set_times(FileTimes::new().set_modified(modified));
    }
    target.sync_all().map_err(at(to))?;
    drop(target);
    if !append {
        index_off(to, copying)?;
        if metadata.permissions().readonly() {
            let mut permissions = fs::metadata(to).map_err(at(to))?.permissions();
            permissions.set_readonly(true);
            fs::set_permissions(to, permissions).map_err(at(to))?;
        }
    }
    Ok(Written { path: to.to_path_buf(), offset, bytes, hash: hasher.finalize() })
}

/// Reads every written file back and compares it; `checked` gets the bytes read so far.
pub(crate) fn verify(
    written: &[Written],
    checked: &mut dyn FnMut(u64),
    cancelled: &dyn Fn() -> bool,
) -> Result<(), CopyError> {
    let mut buffer = vec![0_u8; CHUNK];
    for file in written {
        let mut opened = File::open(&file.path).map_err(at(&file.path))?;
        if opened.metadata().map_err(at(&file.path))?.len() != file.offset + file.bytes {
            return Err(CopyError::Mismatch(file.path.clone()));
        }
        opened.seek(SeekFrom::Start(file.offset)).map_err(at(&file.path))?;
        let mut hasher = blake3::Hasher::new();
        let mut left = file.bytes;
        while left > 0 {
            if cancelled() {
                return Err(CopyError::Cancelled);
            }
            let wanted = buffer.len().min(usize::try_from(left).unwrap_or(usize::MAX));
            let read = opened.read(&mut buffer[..wanted]).map_err(at(&file.path))?;
            if read == 0 {
                return Err(CopyError::Mismatch(file.path.clone()));
            }
            hasher.update(&buffer[..read]);
            left -= read as u64;
            checked(read as u64);
        }
        if hasher.finalize() != file.hash {
            return Err(CopyError::Mismatch(file.path.clone()));
        }
    }
    Ok(())
}

/// Takes back what a move made, newest first; what cannot be removed is named.
pub(crate) fn undo(created: &[Created]) -> Vec<String> {
    let mut left = Vec::new();
    for made in created.iter().rev() {
        let result = match made {
            Created::Folder { path } => match fs::remove_dir_all(path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            },
            Created::File { path } => remove_file(path),
            Created::Appended { path, length } => {
                OpenOptions::new().write(true).open(path).and_then(|file| {
                    file.set_len(*length)?;
                    file.sync_all()
                })
            }
        };
        if let Err(error) = result {
            let path = match made {
                Created::Folder { path }
                | Created::File { path }
                | Created::Appended { path, .. } => path,
            };
            left.push(format!("{}: {error}", path.display()));
        }
    }
    left
}

/// Removes a file, clearing its read-only flag when that is what stands in the way.
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            let mut permissions = fs::metadata(path)?.permissions();
            if !permissions.readonly() {
                return Err(error);
            }
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions)?;
            fs::remove_file(path)
        }
        Err(error) => Err(error),
    }
}
