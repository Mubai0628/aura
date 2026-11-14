//! The team's rows in the execution store. The tables are created with the agent schema; every
//! read and write of them is here.
use crate::{Member, MemberDetails, MemberState, Report, TurnEnd, member_handle};
use aura_domain_common::{SessionId, TurnTaskId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

const MEMBER_COLUMNS: &str = "child_session_id, parent_session_id, handle, spawn_call, state, \
     revision, payload, created_at, updated_at";

/// One report as it was handed in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredReport {
    pub child_session_id: SessionId,
    pub sequence: u32,
    pub turn_task_id: TurnTaskId,
    pub report: Report,
    /// Written by Aura when a turn ended without a final report.
    pub by_turn_end: bool,
    pub created_at: UtcTimestamp,
}

pub fn insert_member(tx: &Transaction<'_>, member: &Member) -> StoreResult<()> {
    if member.revision != 1 || member.state != MemberState::Running {
        return Err(Error::Invalid("sub-agent start"));
    }
    tx.execute(
        &format!(
            "INSERT INTO agent_team_members({MEMBER_COLUMNS}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)"
        ),
        params![
            member.child_session_id.to_string(),
            member.parent_session_id.to_string(),
            member.handle,
            member.spawn_call,
            member.state.as_str(),
            1_i64,
            serde_json::to_string(&member.details)?,
            member.created_at.to_string(),
            member.updated_at.to_string(),
        ],
    )?;
    Ok(())
}

/// Writes `next` over `previous`, one revision on.
pub fn save_member(tx: &Transaction<'_>, previous: &Member, next: &Member) -> StoreResult<()> {
    if next.revision != previous.revision + 1 || next.child_session_id != previous.child_session_id
    {
        return Err(Error::Conflict);
    }
    let changed = tx.execute(
        "UPDATE agent_team_members SET state=?2, revision=?3, payload=?4, updated_at=?5 \
         WHERE child_session_id=?1 AND revision=?6",
        params![
            next.child_session_id.to_string(),
            next.state.as_str(),
            to_sql(next.revision)?,
            serde_json::to_string(&next.details)?,
            next.updated_at.to_string(),
            to_sql(previous.revision)?,
        ],
    )?;
    if changed == 1 { Ok(()) } else { Err(Error::Conflict) }
}

/// Changes what the user lets sub-agent `child`'s commands do without asking, one revision on;
/// a change that leaves them as they were writes nothing.
pub fn change_allowed(
    tx: &Transaction<'_>,
    child: SessionId,
    change: impl FnOnce(&mut crate::Allowances),
) -> StoreResult<crate::Allowances> {
    let current = member_by_child(tx, child)?.ok_or(Error::Invalid("sub-agent"))?;
    let mut next = current.clone();
    change(&mut next.details.allowed);
    if next.details.allowed != current.details.allowed {
        next.revision += 1;
        next.updated_at = UtcTimestamp::now();
        save_member(tx, &current, &next)?;
    }
    Ok(next.details.allowed)
}

/// The member working in conversation `child`, if `child` is a sub-agent's.
pub fn member_by_child(db: &Connection, child: SessionId) -> StoreResult<Option<Member>> {
    one(db, "child_session_id=?1", &child.to_string())
}

pub fn member_by_spawn(db: &Connection, spawn_call: &str) -> StoreResult<Option<Member>> {
    one(db, "spawn_call=?1", spawn_call)
}

pub fn member_by_handle(
    db: &Connection,
    parent: SessionId,
    handle: &str,
) -> StoreResult<Option<Member>> {
    let handle = handle.trim().to_ascii_uppercase();
    db.query_row(
        &format!(
            "SELECT {MEMBER_COLUMNS} FROM agent_team_members \
             WHERE parent_session_id=?1 AND handle=?2"
        ),
        params![parent.to_string(), handle],
        member_row,
    )
    .optional()?
    .transpose()
}

/// The sub-agents conversation `parent` started, oldest first.
pub fn members_of(db: &Connection, parent: SessionId) -> StoreResult<Vec<Member>> {
    let mut statement = db.prepare(&format!(
        "SELECT {MEMBER_COLUMNS} FROM agent_team_members WHERE parent_session_id=?1 \
         ORDER BY created_at, length(handle), handle"
    ))?;
    let rows = statement.query_map([parent.to_string()], member_row)?;
    rows.map(|row| row?).collect()
}

/// Every sub-agent working now, of any conversation, oldest first.
pub fn running_members(db: &Connection) -> StoreResult<Vec<Member>> {
    let mut statement = db.prepare(&format!(
        "SELECT {MEMBER_COLUMNS} FROM agent_team_members WHERE state='running' ORDER BY created_at"
    ))?;
    let rows = statement.query_map([], member_row)?;
    rows.map(|row| row?).collect()
}

/// Records how a turn of `child` ended. A sub-agent still running gave no final report this
/// time: `report` is kept for it under `source`, once, and it waits for its main agent. Returns
/// whether a sub-agent was written.
pub fn record_turn_end(
    tx: &Transaction<'_>,
    child: SessionId,
    end: &TurnEnd,
    report: &Report,
    source: &str,
    at: UtcTimestamp,
) -> StoreResult<bool> {
    let Some(current) = member_by_child(tx, child)? else { return Ok(false) };
    let mut next = current.clone();
    if current.state == MemberState::Running {
        append_report(tx, child, source, end.turn_task_id, report, at)?;
        next.state = MemberState::Reported;
    }
    next.details.last_turn = Some(end.clone());
    next.revision += 1;
    next.updated_at = at;
    save_member(tx, &current, &next)?;
    Ok(true)
}

/// The sequence of `child`'s newest report that ended a piece of its work; 0 before any.
pub fn latest_final_sequence(db: &Connection, child: SessionId) -> StoreResult<u32> {
    let sequence: i64 = db.query_row(
        "SELECT coalesce(max(sequence),0) FROM agent_team_reports \
         WHERE child_session_id=?1 AND status<>'progress'",
        [child.to_string()],
        |row| row.get(0),
    )?;
    from_sql(sequence)
}

/// The handle the next sub-agent of `parent` gets.
pub fn next_handle(db: &Connection, parent: SessionId) -> StoreResult<String> {
    let count: i64 = db.query_row(
        "SELECT count(*) FROM agent_team_members WHERE parent_session_id=?1",
        [parent.to_string()],
        |row| row.get(0),
    )?;
    Ok(member_handle(u32::try_from(count + 1).map_err(|_| Error::Invalid("sub-agent count"))?))
}

/// Keeps a report once: handing in the same `source` again returns the sequence it got.
pub fn append_report(
    tx: &Transaction<'_>,
    child: SessionId,
    source: &str,
    turn: TurnTaskId,
    report: &Report,
    at: UtcTimestamp,
) -> StoreResult<u32> {
    if let Some(sequence) = tx
        .query_row("SELECT sequence FROM agent_team_reports WHERE source=?1", [source], |row| {
            row.get::<_, i64>(0)
        })
        .optional()?
    {
        return from_sql(sequence);
    }
    let sequence: i64 = tx.query_row(
        "SELECT coalesce(max(sequence),0)+1 FROM agent_team_reports WHERE child_session_id=?1",
        [child.to_string()],
        |row| row.get(0),
    )?;
    tx.execute(
        "INSERT INTO agent_team_reports(child_session_id, sequence, source, turn_task_id, status, \
         payload, created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            child.to_string(),
            sequence,
            source,
            turn.to_string(),
            report.status.as_str(),
            serde_json::to_string(report)?,
            at.to_string(),
        ],
    )?;
    from_sql(sequence)
}

/// The reports of `child` after sequence `after`, oldest first.
pub fn reports_after(
    db: &Connection,
    child: SessionId,
    after: u32,
) -> StoreResult<Vec<StoredReport>> {
    let mut statement = db.prepare(
        "SELECT child_session_id, sequence, source, turn_task_id, payload, created_at \
         FROM agent_team_reports WHERE child_session_id=?1 AND sequence>?2 ORDER BY sequence",
    )?;
    let rows = statement.query_map(params![child.to_string(), i64::from(after)], report_row)?;
    rows.map(|row| row?).collect()
}

/// The newest report of `child`.
pub fn latest_report(db: &Connection, child: SessionId) -> StoreResult<Option<StoredReport>> {
    db.query_row(
        "SELECT child_session_id, sequence, source, turn_task_id, payload, created_at \
         FROM agent_team_reports WHERE child_session_id=?1 ORDER BY sequence DESC LIMIT 1",
        [child.to_string()],
        report_row,
    )
    .optional()?
    .transpose()
}

/// The source key of the report Aura writes when the user stops sub-agent `child`.
#[must_use]
pub fn user_stop_source(child: SessionId) -> String {
    format!("user-stop:{child}")
}

/// The source key of the report Aura writes when turn `turn` ended without a final report.
#[must_use]
pub fn turn_end_source(turn: TurnTaskId) -> String {
    format!("turn-end:{turn}")
}

fn one(db: &Connection, condition: &str, value: &str) -> StoreResult<Option<Member>> {
    db.query_row(
        &format!("SELECT {MEMBER_COLUMNS} FROM agent_team_members WHERE {condition}"),
        [value],
        member_row,
    )
    .optional()?
    .transpose()
}

fn member_row(row: &Row<'_>) -> rusqlite::Result<StoreResult<Member>> {
    let fields: (String, String, String, String, String, i64, String, String, String) = (
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    );
    Ok((|| {
        let (child, parent, handle, spawn_call, state, revision, payload, created, updated) =
            fields;
        Ok(Member {
            child_session_id: child.parse().map_err(|_| Error::Invalid("sub-agent session"))?,
            parent_session_id: parent.parse().map_err(|_| Error::Invalid("sub-agent parent"))?,
            handle,
            spawn_call,
            state: MemberState::parse(&state).ok_or(Error::Invalid("sub-agent state"))?,
            revision: u64::try_from(revision).map_err(|_| Error::Invalid("sub-agent revision"))?,
            details: serde_json::from_str::<MemberDetails>(&payload)?,
            created_at: created.parse().map_err(|_| Error::Invalid("sub-agent time"))?,
            updated_at: updated.parse().map_err(|_| Error::Invalid("sub-agent time"))?,
        })
    })())
}

fn report_row(row: &Row<'_>) -> rusqlite::Result<StoreResult<StoredReport>> {
    let fields: (String, i64, String, String, String, String) =
        (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?);
    Ok((|| {
        let (child, sequence, source, turn, payload, created) = fields;
        Ok(StoredReport {
            child_session_id: child.parse().map_err(|_| Error::Invalid("report session"))?,
            sequence: from_sql(sequence)?,
            turn_task_id: turn.parse().map_err(|_| Error::Invalid("report turn"))?,
            report: serde_json::from_str(&payload)?,
            by_turn_end: source.starts_with("turn-end:"),
            created_at: created.parse().map_err(|_| Error::Invalid("report time"))?,
        })
    })())
}

fn to_sql(value: u64) -> StoreResult<i64> {
    i64::try_from(value).map_err(|_| Error::Invalid("sub-agent revision"))
}

fn from_sql(value: i64) -> StoreResult<u32> {
    u32::try_from(value).map_err(|_| Error::Invalid("report sequence"))
}
