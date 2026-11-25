//! The limits and permissions the user set for sub-agents, kept in the execution store.
use crate::{AgentTeamLimits, AgentTeamPermissions};
use aura_domain_common::UtcTimestamp;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_security_contract::PermissionMode;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

/// The limits now and the settings revision: the defaults and 0 until the user saved any. A value
/// outside its range, from another version or a damaged row, is read as the nearest one inside.
pub fn limits(db: &Connection) -> StoreResult<(AgentTeamLimits, u64)> {
    let (limits, _, revision) = settings(db)?;
    Ok((limits, revision))
}

/// The limits, the permissions and the settings revision.
pub fn settings(db: &Connection) -> StoreResult<(AgentTeamLimits, AgentTeamPermissions, u64)> {
    let (stored, revision) = read_stored(db)?;
    Ok((stored.within_range(), stored.permissions(), revision))
}

/// A missing field in an older row keeps collaboration enabled, without changing other settings.
pub fn enabled(db: &Connection) -> StoreResult<bool> {
    Ok(read_stored(db)?.0.enabled)
}

fn read_stored(db: &Connection) -> StoreResult<(Stored, u64)> {
    let row: Option<(i64, String)> = db
        .query_row("SELECT revision, payload FROM agent_team_settings WHERE id=1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()?;
    let Some((revision, payload)) = row else {
        return Ok((Stored::default(), 0));
    };
    let stored: Stored = serde_json::from_str(&payload)?;
    let revision = u64::try_from(revision).map_err(|_| Error::Invalid("limits revision"))?;
    Ok((stored, revision))
}

/// Saves `limits` at the revision the caller read, keeping the permissions. Returns the new
/// revision.
pub fn set_limits(
    tx: &Transaction<'_>,
    limits: &AgentTeamLimits,
    expected: u64,
) -> StoreResult<u64> {
    set_settings(tx, limits, None, expected)
}

/// Saves `limits`, and `permissions` when given, at the revision the caller read. Returns the new
/// revision.
pub fn set_settings(
    tx: &Transaction<'_>,
    limits: &AgentTeamLimits,
    permissions: Option<&AgentTeamPermissions>,
    expected: u64,
) -> StoreResult<u64> {
    set_controls(tx, limits, permissions, None, expected)
}

/// Changes only the supplied controls at the revision the user read.
pub fn set_controls(
    tx: &Transaction<'_>,
    limits: &AgentTeamLimits,
    permissions: Option<&AgentTeamPermissions>,
    enabled: Option<bool>,
    expected: u64,
) -> StoreResult<u64> {
    let (kept, current) = read_stored(tx)?;
    if current != expected {
        return Err(Error::Conflict);
    }
    let next = current.checked_add(1).ok_or(Error::Conflict)?;
    let permissions = permissions.copied().unwrap_or_else(|| kept.permissions());
    let stored = Stored {
        enabled: enabled.unwrap_or(kept.enabled),
        max_running: limits.max_running,
        token_budget: limits.token_budget,
        minutes_per_task: limits.minutes_per_task,
        cpu_percent: limits.cpu_percent,
        worktree_mode: permissions.worktree_mode,
        shared_mode: permissions.shared_mode,
        allow_deletions: permissions.allow_deletions,
        allow_outside_writes: permissions.allow_outside_writes,
        allow_downloads: permissions.allow_downloads,
        main_agent_approves: permissions.main_agent_approves,
    };
    tx.execute(
        "INSERT INTO agent_team_settings(id, revision, payload, updated_at) VALUES(1,?1,?2,?3) \
         ON CONFLICT(id) DO UPDATE SET revision=excluded.revision, payload=excluded.payload, \
         updated_at=excluded.updated_at",
        params![
            i64::try_from(next).map_err(|_| Error::Invalid("limits revision"))?,
            serde_json::to_string(&stored)?,
            UtcTimestamp::now().to_string()
        ],
    )?;
    Ok(next)
}

/// The row's payload. A field another version adds is left out, and one it lacks is the default:
/// a token budget never saved is none.
#[derive(Deserialize, Serialize)]
#[serde(default)]
struct Stored {
    enabled: bool,
    max_running: u32,
    token_budget: Option<u64>,
    minutes_per_task: u32,
    cpu_percent: u32,
    worktree_mode: PermissionMode,
    shared_mode: PermissionMode,
    allow_deletions: bool,
    allow_outside_writes: bool,
    allow_downloads: bool,
    main_agent_approves: bool,
}

impl Default for Stored {
    fn default() -> Self {
        let defaults = AgentTeamLimits::DEFAULT;
        let permissions = AgentTeamPermissions::DEFAULT;
        Self {
            enabled: true,
            max_running: defaults.max_running,
            token_budget: defaults.token_budget,
            minutes_per_task: defaults.minutes_per_task,
            cpu_percent: defaults.cpu_percent,
            worktree_mode: permissions.worktree_mode,
            shared_mode: permissions.shared_mode,
            allow_deletions: permissions.allow_deletions,
            allow_outside_writes: permissions.allow_outside_writes,
            allow_downloads: permissions.allow_downloads,
            main_agent_approves: permissions.main_agent_approves,
        }
    }
}

impl Stored {
    const fn permissions(&self) -> AgentTeamPermissions {
        AgentTeamPermissions {
            worktree_mode: self.worktree_mode,
            shared_mode: self.shared_mode,
            allow_deletions: self.allow_deletions,
            allow_outside_writes: self.allow_outside_writes,
            allow_downloads: self.allow_downloads,
            main_agent_approves: self.main_agent_approves,
        }
    }

    fn within_range(&self) -> AgentTeamLimits {
        let clamp32 = |value: u32, range: std::ops::RangeInclusive<u32>| {
            value.clamp(*range.start(), *range.end())
        };
        AgentTeamLimits {
            max_running: clamp32(self.max_running, AgentTeamLimits::RUNNING),
            token_budget: self.token_budget.map(|budget| {
                budget.clamp(*AgentTeamLimits::TOKENS.start(), *AgentTeamLimits::TOKENS.end())
            }),
            minutes_per_task: clamp32(self.minutes_per_task, AgentTeamLimits::MINUTES),
            cpu_percent: clamp32(self.cpu_percent, AgentTeamLimits::CPU),
        }
    }
}
