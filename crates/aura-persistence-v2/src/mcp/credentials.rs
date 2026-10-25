//! The index of MCP secrets and the OAuth sign-ins. A secret's value lives in the
//! Windows Credential Manager; this table says which server it belongs to, its revision, and
//! whether the value was stored (a row is written before the value and removed after it, so
//! a crash leaves a row Core can clean up, never an unknown value). An OAuth sign-in keeps
//! its tokens encrypted by Core for the current Windows user (DPAPI); this store never sees
//! them in clear.
use aura_domain_common::{CredentialId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError, StoreResult, record};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretState {
    /// Registered; the value may or may not have been written.
    Pending,
    Stored,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretRecord {
    pub secret_id: CredentialId,
    pub server_id: String,
    pub revision: u64,
    pub state: SecretState,
    pub updated_at: UtcTimestamp,
}

fn invalid(what: &'static str) -> PersistenceCoreError {
    PersistenceCoreError::Invalid(what)
}

fn timestamp(text: &str) -> StoreResult<UtcTimestamp> {
    record::decode(&serde_json::to_string(text)?)
}

fn secret_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String, i64, String, String)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
}

fn decode_secret(
    (secret_id, server_id, revision, state, updated_at): (String, String, i64, String, String),
) -> StoreResult<SecretRecord> {
    Ok(SecretRecord {
        secret_id: secret_id.parse().map_err(|_| invalid("MCP secret id"))?,
        server_id,
        revision: u64::try_from(revision).map_err(|_| invalid("MCP secret revision"))?,
        state: match state.as_str() {
            "pending" => SecretState::Pending,
            "stored" => SecretState::Stored,
            _ => return Err(invalid("MCP secret state")),
        },
        updated_at: timestamp(&updated_at)?,
    })
}

const SECRET_COLUMNS: &str = "secret_id,server_id,revision,state,updated_at";

pub fn secret(db: &Connection, secret_id: CredentialId) -> StoreResult<Option<SecretRecord>> {
    db.query_row(
        &format!("SELECT {SECRET_COLUMNS} FROM mcp_secrets WHERE secret_id=?1"),
        [secret_id.to_string()],
        secret_row,
    )
    .optional()?
    .map(decode_secret)
    .transpose()
}

pub fn secrets(db: &Connection) -> StoreResult<Vec<SecretRecord>> {
    let mut query =
        db.prepare(&format!("SELECT {SECRET_COLUMNS} FROM mcp_secrets ORDER BY secret_id"))?;
    let rows = query.query_map([], secret_row)?.collect::<Result<Vec<_>, _>>()?;
    rows.into_iter().map(decode_secret).collect()
}

/// Registers the next value of `secret_id` for `server_id` before it is written: a new ID
/// starts at revision 1; a known one must belong to the same server and advances.
pub fn register_secret(
    tx: &Transaction<'_>,
    secret_id: CredentialId,
    server_id: &str,
    at: UtcTimestamp,
) -> StoreResult<SecretRecord> {
    let revision = match secret(tx, secret_id)? {
        Some(known) if known.server_id != server_id => {
            return Err(invalid("MCP secret belongs to another server"));
        }
        Some(known) => known.revision.checked_add(1).ok_or(invalid("MCP secret revision"))?,
        None => 1,
    };
    let stored = i64::try_from(revision).map_err(|_| invalid("MCP secret revision"))?;
    tx.execute(
        "INSERT INTO mcp_secrets(secret_id,server_id,revision,state,updated_at) VALUES(?1,?2,?3,'pending',?4) ON CONFLICT(secret_id) DO UPDATE SET revision=excluded.revision, state='pending', updated_at=excluded.updated_at",
        params![secret_id.to_string(), server_id, stored, at.to_string()],
    )?;
    secret(tx, secret_id)?.ok_or_else(|| invalid("MCP secret"))
}

/// The value of `revision` was written and verified.
pub fn secret_stored(
    tx: &Transaction<'_>,
    secret_id: CredentialId,
    revision: u64,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let revision = i64::try_from(revision).map_err(|_| invalid("MCP secret revision"))?;
    let changed = tx.execute(
        "UPDATE mcp_secrets SET state='stored', updated_at=?3 WHERE secret_id=?1 AND revision=?2",
        params![secret_id.to_string(), revision, at.to_string()],
    )?;
    if changed == 1 { Ok(()) } else { Err(invalid("MCP secret revision changed")) }
}

pub fn forget_secret(tx: &Transaction<'_>, secret_id: CredentialId) -> StoreResult<()> {
    tx.execute("DELETE FROM mcp_secrets WHERE secret_id=?1", [secret_id.to_string()])?;
    Ok(())
}

/// One server's OAuth sign-in: the authorization server and client it used (no secret) and
/// its tokens, encrypted by Core.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthSession {
    pub server_id: String,
    /// The trusted configuration the sign-in was made for; tokens are never used for another.
    pub config_fingerprint: String,
    /// Names the tokens toward the network gateway; the revision advances on every refresh.
    pub credential_id: CredentialId,
    pub revision: u64,
    pub authorization: serde_json::Value,
    pub protected_tokens: Vec<u8>,
    pub expires_at: Option<UtcTimestamp>,
    pub updated_at: UtcTimestamp,
}

pub fn save_oauth(tx: &Transaction<'_>, session: &OAuthSession) -> StoreResult<()> {
    let revision = i64::try_from(session.revision).map_err(|_| invalid("OAuth revision"))?;
    tx.execute(
        "INSERT INTO mcp_oauth_sessions(server_id,config_fingerprint,credential_id,revision,authorization,protected_tokens,expires_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(server_id) DO UPDATE SET config_fingerprint=excluded.config_fingerprint, credential_id=excluded.credential_id, revision=excluded.revision, authorization=excluded.authorization, protected_tokens=excluded.protected_tokens, expires_at=excluded.expires_at, updated_at=excluded.updated_at",
        params![
            session.server_id,
            session.config_fingerprint,
            session.credential_id.to_string(),
            revision,
            session.authorization.to_string(),
            session.protected_tokens,
            session.expires_at.map(|at| at.to_string()),
            session.updated_at.to_string(),
        ],
    )?;
    Ok(())
}

pub fn oauth(db: &Connection, server_id: &str) -> StoreResult<Option<OAuthSession>> {
    type Row = (String, String, String, i64, String, Vec<u8>, Option<String>, String);
    let row: Option<Row> = db
        .query_row(
            "SELECT server_id,config_fingerprint,credential_id,revision,authorization,protected_tokens,expires_at,updated_at FROM mcp_oauth_sessions WHERE server_id=?1",
            [server_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .optional()?;
    let Some((server_id, fingerprint, credential, revision, authorization, tokens, expires, at)) =
        row
    else {
        return Ok(None);
    };
    Ok(Some(OAuthSession {
        server_id,
        config_fingerprint: fingerprint,
        credential_id: credential.parse().map_err(|_| invalid("OAuth credential id"))?,
        revision: u64::try_from(revision).map_err(|_| invalid("OAuth revision"))?,
        authorization: serde_json::from_str(&authorization)?,
        protected_tokens: tokens,
        expires_at: expires.as_deref().map(timestamp).transpose()?,
        updated_at: timestamp(&at)?,
    }))
}

pub fn forget_oauth(tx: &Transaction<'_>, server_id: &str) -> StoreResult<()> {
    tx.execute("DELETE FROM mcp_oauth_sessions WHERE server_id=?1", [server_id])?;
    Ok(())
}

/// Every server with a sign-in, for cleaning up after servers that were removed.
pub fn oauth_servers(db: &Connection) -> StoreResult<Vec<String>> {
    let mut query = db.prepare("SELECT server_id FROM mcp_oauth_sessions ORDER BY server_id")?;
    Ok(query.query_map([], |row| row.get(0))?.collect::<Result<Vec<_>, _>>()?)
}
