use crate::{PersistenceCoreError, StoreResult};
use serde::{Serialize, de::DeserializeOwned};

/// Canonical persisted domain payload. SQL columns own query keys; callers
/// validate domain semantics before storage, and readers decode exact versions.
pub fn encode<T: Serialize>(value: &T) -> StoreResult<String> {
    let bytes = aura_domain_common::jcs::canonical_json_jcs(value)
        .map_err(|_| PersistenceCoreError::Invalid("canonical payload"))?;
    String::from_utf8(bytes).map_err(|_| PersistenceCoreError::Invalid("UTF-8 payload"))
}

pub fn decode<T: DeserializeOwned>(value: &str) -> StoreResult<T> {
    Ok(serde_json::from_str(value)?)
}

pub fn changed(rows: usize) -> StoreResult<()> {
    if rows == 1 { Ok(()) } else { Err(PersistenceCoreError::Conflict) }
}

pub fn sql_u64(value: u64) -> StoreResult<i64> {
    i64::try_from(value).map_err(|_| PersistenceCoreError::Invalid("SQLite integer range"))
}

/// Fixed nanosecond precision preserves ordering at exact-second boundaries.
pub fn sql_time(value: aura_domain_common::UtcTimestamp) -> String {
    let wire = value.to_string();
    let utc = wire.strip_suffix('Z').expect("UtcTimestamp formats UTC");
    let (seconds, fraction) = utc.split_once('.').unwrap_or((utc, ""));
    format!("{seconds}.{fraction:0<9}Z")
}
