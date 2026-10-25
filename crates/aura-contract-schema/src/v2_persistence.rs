use aura_domain_common::{ContentHash, ToolWireVersionV1};
use aura_persistence_core::import_report::{ImportDisposition, ImportItem, ImportReport};

pub fn v2_import_report_fixture() -> ImportReport {
    ImportReport {
        schema_version: ToolWireVersionV1,
        source_fingerprint: ContentHash::blake3(b"synthetic R02 import"),
        applied: false,
        already_applied: false,
        items: [
            ImportDisposition::Imported,
            ImportDisposition::Skipped,
            ImportDisposition::Invalid,
            ImportDisposition::RequiresUserAction,
            ImportDisposition::Conflict,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, disposition)| ImportItem {
            kind: "fixture".into(),
            source_key: format!("fixture-{index}"),
            disposition,
            reason_code: "synthetic_coverage".into(),
            record_count: 1,
        })
        .collect(),
    }
}
