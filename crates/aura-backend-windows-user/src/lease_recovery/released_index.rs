//! Trusted index of fully released lease journals.
//!
//! A released journal is historical evidence: no stop proof is read from it, no
//! ACL is restored by it, and nothing it says can dispatch a command. Recovery
//! still re-read every one of them on every cold open — four full reads and
//! three full parses of the whole installation history — only to reconfirm a
//! fact that cannot change: a released journal is append-complete.
//!
//! This index records that fact once. It is only believed while the journal still
//! *is* the file the entry was built from, which takes three independent facts:
//! the object identity (volume serial and file id), its exact size, and the
//! volume's change generation for it read through a handle that excludes writers.
//! The identity and size alone are not enough — a rewrite that keeps both is the
//! same file with different bytes — and the generation alone is not enough
//! either: a write handle that stays open advances the generation only once, so
//! the value must be read in a state where no writer can be holding the object.
//! Anything that does not match — a missing entry, a replaced identity, a size or
//! generation change, a journal that never settled, an index from another
//! installation or another format — falls back to reading the journal itself. A
//! corrupt or foreign index is simply no index.
//!
//! The index lives beside the credentials under `private/`, whose ACL recovery
//! verifies on every open, so it can be written only by the same installation
//! owner that can write the journals themselves; it grants that writer no power
//! it did not already have.
use anyhow::Result;
use aura_domain_common::ContentHash;
use aura_windows_platform::ChangeEpoch;
use aura_windows_user_contract::ExecutionFileBinding;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

const SCHEMA: &str = "windows-user-released-lease-index-v1";
/// Far beyond any real history; the cap keeps a corrupt writer from growing an
/// unbounded cache, not from doing harm — a missing entry costs one full read.
const MAX_ENTRIES: usize = 8192;
const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;

/// One released journal's cached facts, valid only while the file on disk is
/// still the file these facts were read from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReleasedEntry {
    pub volume_serial: String,
    pub file_id: [u8; 16],
    pub size: u64,
    pub sha256: ContentHash,
    /// The volume's change generation for this journal, read through the same
    /// handle that produced `sha256`. A later open may reuse the cached facts only
    /// while a writer-excluding witness still reports this exact value.
    pub change_usn: u64,
    /// Scope roots the journal declared, reused for scratch cleanup. Every one
    /// is re-verified against the live object before anything is removed.
    pub roots: Vec<ExecutionFileBinding>,
}

/// Whether the live journal at `path` is still the file this entry was built
/// from.
///
/// The witness open is the load-bearing part. Identity and size alone accept a
/// same-length rewrite of the same file; the change generation alone accepts a
/// rewrite that is still going on, because a write handle that stays open
/// advances the volume's counter only once for the whole session. An open that
/// excludes writers turns that session into a refusal instead of an answer.
impl ReleasedEntry {
    pub(crate) fn matches(&self, path: &Path) -> bool {
        let Ok(aura_windows_platform::ChangeWitnessOutcome::Witnessed(witness)) =
            aura_windows_platform::witness_change_generation(path)
        else {
            return false;
        };
        let identity = witness.identity();
        identity.volume_serial_number.to_string() == self.volume_serial
            && identity.file_id == self.file_id
            && witness.byte_length() == self.size
            && witness.change_usn() == self.change_usn
    }
}

#[derive(Default)]
pub(crate) struct ReleasedIndex {
    installation: String,
    entries: BTreeMap<String, ReleasedEntry>,
    /// The volume liveness anchor stamped by the pass that wrote this index. A
    /// later pass requires the counter to have moved over its own sentinel write
    /// before it believes any entry, which is how a disabled or restarted change
    /// log stops being mistaken for "nothing changed".
    epoch: Option<ChangeEpoch>,
    dirty: bool,
}

impl ReleasedIndex {
    /// Load the installation's index. Any doubt — absent, unreadable, oversized,
    /// malformed, from another installation or another format — means an empty
    /// index, which means the full recovery path runs as if no cache existed.
    pub(crate) fn load(root: &Path, installation: &str) -> Self {
        let empty = || Self { installation: installation.to_owned(), ..Default::default() };
        let Ok(bytes) = std::fs::read(path(root)) else {
            return empty();
        };
        if bytes.len() as u64 > MAX_INDEX_BYTES {
            return empty();
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            schema: String,
            installation: String,
            journals: BTreeMap<String, ReleasedEntry>,
            epoch: Option<ChangeEpochWire>,
        }
        #[derive(Deserialize, Serialize)]
        #[serde(deny_unknown_fields)]
        struct ChangeEpochWire {
            volume_serial: u64,
            usn: u64,
        }
        let Ok(wire) = serde_json::from_slice::<Wire>(&bytes) else {
            return empty();
        };
        if wire.schema != SCHEMA || wire.installation != installation {
            return empty();
        }
        Self {
            installation: installation.to_owned(),
            entries: wire.journals,
            epoch: wire
                .epoch
                .map(|epoch| ChangeEpoch { volume_serial: epoch.volume_serial, usn: epoch.usn }),
            dirty: false,
        }
    }

    /// The cached facts for one journal name, when present.
    pub(crate) fn entry(&self, name: &str) -> Option<&ReleasedEntry> {
        self.entries.get(name)
    }

    /// Whether there is anything worth verifying at all. A pass with no entries
    /// does not stamp the liveness anchor, so an installation with an empty
    /// history pays nothing for a proof it does not need.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The liveness anchor recorded by the pass that wrote this index.
    pub(crate) fn epoch(&self) -> Option<ChangeEpoch> {
        self.epoch
    }

    /// Record this pass's liveness anchor, so the next pass can require the
    /// counter to have moved again.
    pub(crate) fn set_epoch(&mut self, epoch: ChangeEpoch) {
        if self.epoch != Some(epoch) {
            self.epoch = Some(epoch);
            self.dirty = true;
        }
    }

    /// Cache one journal that was fully read this recovery and found released.
    pub(crate) fn insert(&mut self, name: String, entry: ReleasedEntry) {
        if self.entries.len() >= MAX_ENTRIES {
            return;
        }
        self.entries.insert(name, entry);
        self.dirty = true;
    }

    /// Forget journals that no longer exist; returns whether anything changed.
    pub(crate) fn retain(&mut self, present: &std::collections::BTreeSet<String>) {
        let before = self.entries.len();
        self.entries.retain(|name, _| present.contains(name));
        self.dirty = self.dirty || self.entries.len() != before;
    }

    /// Persist the index atomically (write, flush, rename). Only called when
    /// something changed; an empty history writes an empty-but-valid index so a
    /// later load does not mistake absence for corruption.
    pub(crate) fn store(&self, root: &Path) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        #[derive(Serialize)]
        struct Wire<'a> {
            schema: &'a str,
            installation: &'a str,
            journals: &'a BTreeMap<String, ReleasedEntry>,
            epoch: Option<ChangeEpochWire>,
        }
        #[derive(Serialize)]
        struct ChangeEpochWire {
            volume_serial: u64,
            usn: u64,
        }
        let bytes = serde_json::to_vec(&Wire {
            schema: SCHEMA,
            installation: &self.installation,
            journals: &self.entries,
            epoch: self.epoch.map(|epoch| ChangeEpochWire {
                volume_serial: epoch.volume_serial,
                usn: epoch.usn,
            }),
        })?;
        let final_path = path(root);
        let staging = staging_path(root);
        {
            let mut file = std::fs::File::create(&staging)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        std::fs::rename(&staging, &final_path)?;
        Ok(())
    }
}

fn path(root: &Path) -> PathBuf {
    root.join("private/execution-leases-index.json")
}

fn staging_path(root: &Path) -> PathBuf {
    root.join("private/execution-leases-index.json.tmp")
}
