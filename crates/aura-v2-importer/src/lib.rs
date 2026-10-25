#![forbid(unsafe_code)]

mod configurations;
mod publish;
mod source;
pub use publish::{ImportFault, apply, resume};
pub use source::{ImportInput, PreparedImport, backup_legacy, preview};

pub(crate) use aura_persistence_core::import_report::{
    ImportDisposition, ImportItem, ImportReport,
};
pub(crate) use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
pub(crate) fn item(
    report: &mut ImportReport,
    kind: &str,
    key: &str,
    disposition: ImportDisposition,
    reason: &str,
) {
    report.items.push(ImportItem {
        kind: kind.into(),
        source_key: blake3::hash(key.as_bytes()).to_hex().to_string(),
        disposition,
        reason_code: reason.into(),
        record_count: 1,
    });
}
