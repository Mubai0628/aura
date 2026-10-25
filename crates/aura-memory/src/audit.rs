//! Metadata-only records beside each independent derived SQLite cache, so that
//! rebuilding or deleting SQLite loses neither history nor its provenance. History
//! lives in bounded monthly segments; no body or credential is stored in them, and
//! no absent Vault is created for them. (`journal` keeps an unfinished batch's
//! prior bytes beside them only until that batch settles.)
use crate::{MemoryError, MemoryHistory, MemoryResult};
use aura_domain_common::{UtcTimestamp, WorkspacePath};
use aura_fs_tools::vault::{VaultChange, VaultFileSystem, VaultRead};
use std::{collections::BTreeSet, path::Path};

/// Below the 1 MiB file bound with room for a further full batch of records.
const SEGMENT_BYTES: usize = 640 * 1024;
/// A safety bound on files read at once; monthly segments stay far below it.
const MAX_FILES: usize = 10_000;

pub(crate) struct AuditLog {
    pub(crate) fs: VaultFileSystem,
}
impl AuditLog {
    pub fn new(cache: &Path) -> MemoryResult<Self> {
        let stem =
            cache.file_stem().ok_or(MemoryError::Invalid("cache filename"))?.to_string_lossy();
        let root = cache.with_file_name(format!("{stem}.records"));
        Ok(Self { fs: VaultFileSystem::new(root)? })
    }
    /// Append to the current month's newest segment, or start the next one. A
    /// record already present there (a settled journal repeating) is not added twice.
    pub fn append(&self, records: &[MemoryHistory]) -> MemoryResult<()> {
        if records.is_empty() {
            return Ok(());
        }
        let now = UtcTimestamp::now().value();
        let month = format!("{:04}-{:02}", now.year(), u8::from(now.month()));
        let (sequence, latest) = self.latest_segment(&month)?;
        let mut kept = match &latest {
            Some(read) => decode(&read.bytes)?,
            None => vec![],
        };
        let known = kept.iter().map(record_hash).collect::<MemoryResult<BTreeSet<_>>>()?;
        let mut fresh = vec![];
        for record in records {
            if !known.contains(&record_hash(record)?) {
                fresh.push(record.clone());
            }
        }
        if fresh.is_empty() {
            return Ok(());
        }
        kept.extend(fresh.iter().cloned());
        let combined = encode(&kept)?;
        let change = match latest {
            Some(read) if combined.len() <= SEGMENT_BYTES => VaultChange {
                path: segment(&month, sequence)?,
                expected_hash: Some(read.hash),
                bytes: Some(combined),
            },
            _ => VaultChange {
                path: segment(&month, sequence + 1)?,
                expected_hash: None,
                bytes: Some(encode(&fresh)?),
            },
        };
        self.fs.apply(&[change])?;
        Ok(())
    }
    fn latest_segment(&self, month: &str) -> MemoryResult<(u32, Option<VaultRead>)> {
        let root = self.fs.root().join("history");
        let mut sequence = 0;
        if root.try_exists()? {
            for item in std::fs::read_dir(root)? {
                let name = item?.file_name().to_string_lossy().into_owned();
                if let Some(number) = name
                    .strip_prefix(month)
                    .and_then(|rest| rest.strip_prefix('-'))
                    .and_then(|rest| rest.strip_suffix(".json"))
                    .and_then(|number| number.parse::<u32>().ok())
                {
                    sequence = sequence.max(number);
                }
            }
        }
        if sequence == 0 {
            return Ok((0, None));
        }
        Ok((sequence, Some(self.fs.read(&segment(month, sequence)?)?)))
    }
    /// Every record once, oldest first. Earlier releases wrote one file per batch;
    /// those files are read the same way.
    pub fn all(&self) -> MemoryResult<Vec<MemoryHistory>> {
        let root = self.fs.root().join("history");
        if !root.try_exists()? {
            return Ok(vec![]);
        }
        let mut seen = BTreeSet::new();
        let mut records = vec![];
        let mut files = 0;
        for item in std::fs::read_dir(root)? {
            let item = item?;
            if item.path().extension().is_none_or(|e| e != "json") {
                continue;
            }
            files += 1;
            if files > MAX_FILES {
                return Err(MemoryError::Invalid("history file bound exceeded"));
            }
            let path =
                WorkspacePath::parse(format!("history/{}", item.file_name().to_string_lossy()))
                    .map_err(|_| MemoryError::Invalid("history path"))?;
            for record in decode(&self.fs.read(&path)?.bytes)? {
                if seen.insert(record_hash(&record)?) {
                    records.push(record);
                }
            }
        }
        records.sort_by_key(|r| r.at);
        Ok(records)
    }
}

fn segment(month: &str, sequence: u32) -> MemoryResult<WorkspacePath> {
    WorkspacePath::parse(format!("history/{month}-{sequence:04}.json"))
        .map_err(|_| MemoryError::Invalid("history path"))
}
fn record_hash(record: &MemoryHistory) -> MemoryResult<String> {
    serde_json::to_vec(record)
        .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
        .map_err(|_| MemoryError::Invalid("history metadata"))
}
fn encode(records: &[MemoryHistory]) -> MemoryResult<Vec<u8>> {
    serde_json::to_vec(records).map_err(|_| MemoryError::Invalid("history metadata"))
}
fn decode(bytes: &[u8]) -> MemoryResult<Vec<MemoryHistory>> {
    serde_json::from_slice(bytes).map_err(|_| MemoryError::Invalid("history metadata"))
}
