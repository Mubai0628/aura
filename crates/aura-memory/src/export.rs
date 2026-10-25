//! Frozen export data lives only in this process, not a cross-Vault database.
use crate::*;
use aura_domain_common::ContentHash;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
#[derive(Default)]
pub(crate) struct Exports {
    pending: BTreeMap<String, Pending>,
}
struct Pending {
    at: Instant,
    files: Vec<File>,
}
struct File {
    entry: MemoryExportEntry,
    bytes: Arc<Vec<u8>>,
}
impl Exports {
    fn prune(&mut self) {
        self.pending.retain(|_, p| p.at.elapsed() < Duration::from_secs(600));
    }
}
impl MemoryService {
    pub fn prepare_export(&self, vault: &Vault) -> MemoryResult<MemoryExportHandle> {
        let mut exports = self.exports.lock().unwrap_or_else(|e| e.into_inner());
        exports.prune();
        if exports.pending.len() >= 4 {
            return Err(MemoryError::Invalid("four exports already pending"));
        }
        let state = vault.lock();
        let mut files = vec![];
        let mut total_bytes = 0u64;
        for entry in state.fs.list_all(10000)? {
            let bytes = if entry.directory { vec![] } else { state.fs.read(&entry.path)?.bytes };
            total_bytes += bytes.len() as u64;
            if total_bytes > 32 * 1024 * 1024 {
                return Err(MemoryError::Invalid("export exceeds 32 MiB; no partial archive"));
            }
            files.push(File {
                entry: MemoryExportEntry {
                    path: entry.path.as_str().into(),
                    directory: entry.directory,
                    length: bytes.len() as u64,
                    sha256: ContentHash::sha256(&bytes).hex,
                },
                bytes: Arc::new(bytes),
            });
        }
        let id =
            ContentHash::blake3(aura_domain_common::MemoryId::new().to_string().as_bytes()).hex;
        let handle =
            MemoryExportHandle { export_id: id.clone(), entries: files.len() as u32, total_bytes };
        exports.pending.insert(id, Pending { at: Instant::now(), files });
        Ok(handle)
    }
    pub fn export_entry(
        &self,
        input: &MemoryExportEntryRequest,
    ) -> MemoryResult<MemoryExportEntry> {
        let mut exports = self.exports.lock().unwrap_or_else(|e| e.into_inner());
        exports.prune();
        Ok(exports
            .pending
            .get(&input.export_id)
            .and_then(|p| p.files.get(input.index as usize))
            .ok_or(MemoryError::NotFound)?
            .entry
            .clone())
    }
    pub fn export_chunk(
        &self,
        input: &MemoryExportChunkRequest,
    ) -> MemoryResult<MemoryExportChunk> {
        if !(1..=65536).contains(&input.max_bytes) {
            return Err(MemoryError::Invalid("export chunk bound"));
        }
        let mut exports = self.exports.lock().unwrap_or_else(|e| e.into_inner());
        exports.prune();
        let file = exports
            .pending
            .get(&input.export_id)
            .and_then(|p| p.files.get(input.index as usize))
            .ok_or(MemoryError::NotFound)?;
        let offset =
            usize::try_from(input.offset).map_err(|_| MemoryError::Invalid("export offset"))?;
        if offset > file.bytes.len() {
            return Err(MemoryError::Invalid("export offset"));
        }
        let end = (offset + input.max_bytes as usize).min(file.bytes.len());
        Ok(MemoryExportChunk {
            offset: input.offset,
            next_offset: end as u64,
            bytes: file.bytes[offset..end].to_vec(),
            eof: end == file.bytes.len(),
        })
    }
    pub fn close_export(&self, input: &MemoryExportCloseRequest) -> MemoryDesktopActionResult {
        self.exports.lock().unwrap_or_else(|e| e.into_inner()).pending.remove(&input.export_id);
        MemoryDesktopActionResult { completed: true, message: "导出快照已释放".into() }
    }
}
