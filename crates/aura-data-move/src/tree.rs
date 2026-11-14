//! Copying a whole folder to another place and checking the copy, for what moves besides the data
//! (the installation's folder): each file is hashed as it is read and read back once all are
//! copied, as a data move does, and what the copy made can be taken back.
use std::path::Path;

use crate::{
    copy::{self, Copying, Written},
    journal::Created,
    plan::{Item, Part},
};

/// One folder's copy: what it made at the destination and every file it wrote.
#[derive(Debug, Default)]
pub struct TreeCopy {
    created: Vec<Created>,
    written: Vec<Written>,
}

impl TreeCopy {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Copies what `from` holds into `to`, which may already be there, leaving out the names in
    /// `skip` directly under `from`. `copied` gets the bytes copied so far.
    pub fn copy(
        &mut self,
        from: &Path,
        to: &Path,
        skip: &[String],
        copied: &mut dyn FnMut(u64),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        // The part names a data move's piece; a copy of a folder of its own reads only the paths.
        let item = Item {
            part: Part::Data,
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            skip: skip.to_vec(),
            skip_caches: false,
            append: false,
            bytes: 0,
            files: 0,
        };
        let created = &mut self.created;
        let mut made = |made: Created| {
            created.push(made);
            Ok::<(), std::io::Error>(())
        };
        let mut copying = Copying { made: &mut made, copied, cancelled, not_indexed: false };
        let written = copy::copy_item(&item, &mut copying).map_err(|error| error.to_string())?;
        self.written.extend(written);
        Ok(())
    }

    /// Reads every copied file back and compares its size and hash.
    pub fn verify(
        &self,
        checked: &mut dyn FnMut(u64),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        copy::verify(&self.written, checked, cancelled).map_err(|error| error.to_string())
    }

    /// The bytes the copy wrote.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.written.iter().map(|file| file.bytes).sum()
    }

    /// Takes back what the copy made, newest first; what could not be removed is named.
    #[must_use]
    pub fn undo(&self) -> Vec<String> {
        copy::undo(&self.created)
    }
}
