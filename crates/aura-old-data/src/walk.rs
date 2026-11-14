//! Reading folders without following links.
use std::{
    fs, io,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    time::SystemTime,
};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// A folder's entries by name; an empty list for a folder that is not there.
pub(crate) fn entries(folder: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let listed = match fs::read_dir(folder) {
        Ok(listed) => listed,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    listed
        .map(|entry| {
            let entry = entry?;
            Ok((entry.file_name().to_string_lossy().into_owned(), entry.path()))
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Measured {
    pub bytes: u64,
    /// The newest change to anything inside.
    pub modified: Option<SystemTime>,
    /// A link, a junction or another reparse point is inside; it was not followed.
    pub link: bool,
}

impl Measured {
    fn add(&mut self, other: Self) {
        self.bytes += other.bytes;
        self.modified = self.modified.max(other.modified);
        self.link |= other.link;
    }
}

/// What `path` holds, file or folder, all the way down.
pub(crate) fn measure(path: &Path) -> io::Result<Measured> {
    let metadata = fs::symlink_metadata(path)?;
    let mut measured = Measured { modified: metadata.modified().ok(), ..Measured::default() };
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        measured.link = true;
        return Ok(measured);
    }
    if !metadata.is_dir() {
        measured.bytes = metadata.len();
        return Ok(measured);
    }
    for (_, inside) in entries(path)? {
        measured.add(measure(&inside)?);
    }
    Ok(measured)
}

/// Everything `paths` hold together; `None` when one cannot be read.
pub(crate) fn measure_all(paths: &[PathBuf]) -> Option<Measured> {
    let mut total = Measured::default();
    for path in paths {
        total.add(measure(path).ok()?);
    }
    Some(total)
}

/// Whether `folder` holds nothing; `false` when it cannot be read.
pub(crate) fn is_empty_folder(folder: &Path) -> bool {
    fs::read_dir(folder).is_ok_and(|mut entries| entries.next().is_none())
}
