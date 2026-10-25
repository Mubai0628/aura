use super::{MigrationFaultPoint, PersistenceError, UtcTimestamp};
use rusqlite::Transaction;

const REBIND_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS builtin_credential_rebind_required(
    tool_instance_id TEXT PRIMARY KEY REFERENCES tool_instances(tool_instance_id),
    reason TEXT NOT NULL
);
CREATE TABLE builtin_credential_rebind_v50(
    tool_instance_id TEXT PRIMARY KEY REFERENCES tool_instances(tool_instance_id),
    reason TEXT NOT NULL CHECK(reason = 'credential_slot_changed')
);
INSERT INTO builtin_credential_rebind_v50
SELECT tool_instance_id,CASE
    WHEN reason = '凭据槽或放置方式已变化。请在工具设置中重新绑定凭据，并核对允许网站。'
    THEN 'credential_slot_changed' ELSE reason END
FROM builtin_credential_rebind_required;
DROP TABLE builtin_credential_rebind_required;
ALTER TABLE builtin_credential_rebind_v50 RENAME TO builtin_credential_rebind_required;
";

pub(super) fn apply(
    tx: &Transaction<'_>,
    current: i64,
    target: i64,
    fault: Option<MigrationFaultPoint>,
) -> Result<(), PersistenceError> {
    if current < 49 && target >= 49 {
        let schema = crate::tool_credential_journal::TOOL_CREDENTIAL_JOURNAL_SCHEMA;
        tx.execute_batch(schema)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 48 {
            tx.execute_batch("THIS IS AN INJECTED INVALID TOOL SECRET JOURNAL MIGRATION")?;
        }
        record(tx, 49, "tool_credential_mutations", schema)?;
    }
    if current < 50 && target >= 50 {
        tx.execute_batch(REBIND_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 49 {
            tx.execute_batch("THIS IS AN INJECTED INVALID BUILTIN REBIND MIGRATION")?;
        }
        record(tx, 50, "builtin_credential_rebind_required", REBIND_SCHEMA)?;
    }
    Ok(())
}

fn record(
    tx: &Transaction<'_>,
    version: i64,
    name: &str,
    schema: &str,
) -> Result<(), PersistenceError> {
    tx.execute(
        "INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES (?1,?2,?3,?4)",
        (
            version,
            name,
            blake3::hash(schema.as_bytes()).to_hex().to_string(),
            UtcTimestamp::now().to_string(),
        ),
    )?;
    Ok(())
}
