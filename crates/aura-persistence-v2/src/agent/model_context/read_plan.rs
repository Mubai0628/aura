use super::*;
use rusqlite::{OptionalExtension, params};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

/// Extra room to finish materializing the adopted prefix and a new group before compaction.
const MAX_READ_HEADROOM: u64 = 4 * 1024 * 1024;

struct AdoptedHistory {
    cutoff: i64,
    events: BTreeSet<EventId>,
    results: BTreeMap<String, ResultProjection>,
}

struct ResultProjection {
    bytes: i64,
    reference: Option<String>,
}

impl AdoptedHistory {
    fn load(
        db: &Connection,
        turn: &TurnTask,
        checkpoint: Option<ContextProjectionSnapshotId>,
    ) -> StoreResult<Option<Self>> {
        let Some(id) = crate::context::latest_adopted_in_scope(db, turn)? else {
            return Ok(None);
        };
        let row = db
            .query_row(
                "SELECT CAST(json_extract(c.payload,'$.history_cutoff') AS INTEGER),
                    (SELECT json_group_array(json_extract(value,'$.source_event_id'))
                     FROM json_each(c.payload,'$.conversation_items')),
                    (SELECT json_extract(src.value,'$.context_snapshot_id')
                     FROM json_each(c.payload,'$.sections') section,
                          json_each(section.value,'$.source_refs') src
                     WHERE json_extract(section.value,'$.kind')='historical_snapshot_summary'
                     AND json_extract(src.value,'$.type')='compaction' LIMIT 1)
             FROM context_snapshots c WHERE c.id=?1",
                [&id],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((Some(cutoff), raw, previous)) = row else { return Ok(None) };
        if previous != checkpoint.map(|id| id.to_string()) {
            return Ok(None);
        }
        let Ok(events) = serde_json::from_str(&raw) else { return Ok(None) };
        let mut query = db.prepare(
            "SELECT json_extract(item.value,'$.source_event_id'),
                    length(CAST(json_extract(item.value,'$.content') AS BLOB)),
                    CASE WHEN substr(json_extract(item.value,'$.content'),1,length(?2))=?2
                         OR substr(json_extract(item.value,'$.content'),1,length(?3))=?3
                         THEN json_extract(item.value,'$.content') END
             FROM context_snapshots c, json_each(c.payload,'$.conversation_items') item
             WHERE c.id=?1 AND json_extract(item.value,'$.type')='tool_result'",
        )?;
        let results = query
            .query_map(
                params![
                    id,
                    aura_context_domain::TOOL_RESULT_REFERENCE_PREFIX,
                    "[Earlier tool result cleared to save context:"
                ],
                |r| Ok((r.get(0)?, ResultProjection { bytes: r.get(1)?, reference: r.get(2)? })),
            )?
            .collect::<Result<_, _>>()?;
        Ok(Some(Self { cutoff, events, results }))
    }

    fn was_deferred(&self, id: EventId, sequence: i64) -> bool {
        sequence <= self.cutoff && !self.events.contains(&id)
    }
}

#[derive(Default)]
pub(crate) struct ReadPlan {
    skip: BTreeSet<EventId>,
    pub(super) deferred: usize,
    pub(super) deferred_range: Option<(i64, i64)>,
    pub(super) covered: usize,
    pub(super) selected_bytes: u64,
    pub(super) compaction_pressure: bool,
    pub(super) missing_adopted: BTreeSet<EventId>,
    remaining_bytes: Option<u64>,
    recent_boundary: bool,
    adopted: Option<AdoptedHistory>,
}

impl ReadPlan {
    pub(crate) fn omits(&self, id: EventId) -> bool {
        self.skip.contains(&id)
    }

    pub(super) fn result_reference(&self, event: &str) -> Option<&str> {
        self.adopted.as_ref()?.results.get(event)?.reference.as_deref()
    }

    pub(super) fn for_turn(
        db: &Connection,
        turn: &TurnTask,
        input_budget: u64,
        checkpoint: Option<&crate::context::compaction::Compaction>,
    ) -> StoreResult<Self> {
        let mut plan = Self {
            skip: checkpoint
                .into_iter()
                .flat_map(|r| r.excluded_event_ids.iter().copied())
                .collect(),
            adopted: AdoptedHistory::load(db, turn, checkpoint.map(|r| r.id))?,
            ..Self::default()
        };
        plan.covered = plan.skip.len();
        let soft_limit = input_budget.saturating_mul(4);
        let mut remaining = if plan.adopted.is_some() {
            soft_limit.saturating_add(soft_limit.min(MAX_READ_HEADROOM))
        } else {
            soft_limit
        };
        let mut required = BTreeSet::from([turn.root_user_event_id]);
        required.extend(super::super::query::events(db, turn.turn_task_id)?);
        required.extend(
            super::super::inbox::consumed(db, turn.turn_task_id)?
                .into_iter()
                .flat_map(|g| g.user_event_ids),
        );
        let mut query = db.prepare(
            "SELECT e.sequence,e.event_id,NULL AS batch,0 AS active FROM events e
             WHERE e.owner=?1 AND e.kind='agent.user_message' AND e.sequence < COALESCE(
                 (SELECT MIN(n.sequence) FROM turn_tasks t JOIN events n ON n.event_id=json_extract(t.payload,'$.root_user_event_id') WHERE t.session_id=?2 AND t.rowid>(SELECT rowid FROM turn_tasks WHERE id=?4)),9223372036854775807)
             UNION ALL
             SELECT e.sequence,e.event_id,json_extract(s.payload,'$.state.tool_batch_id'),s.agent_run_id=?3
             FROM turn_tasks t JOIN model_steps s ON s.agent_run_id=json_extract(t.payload,'$.agent_run_id')
             JOIN events e ON e.event_id=json_extract(s.payload,'$.state.assistant_event_id')
             WHERE t.session_id=?2 AND t.rowid<=(SELECT rowid FROM turn_tasks WHERE id=?4) AND s.state='completed'
               AND (s.agent_run_id=?3 OR NOT EXISTS(
                   SELECT 1 FROM agent_collaboration_deliveries WHERE target_run_id IN (?3,s.agent_run_id)))
             ORDER BY 1 DESC"
        )?;
        let mut events = query.query(params![
            format!("agent-user:{}", turn.session_id),
            turn.session_id.to_string(),
            turn.agent_run_id.to_string(),
            turn.turn_task_id.to_string()
        ])?;
        let mut event_size =
            db.prepare("SELECT length(CAST(payload AS BLOB)) FROM events WHERE event_id=?1")?;
        let mut result_sizes = db.prepare(
            "SELECT e.event_id,o.byte_length FROM events e
             JOIN artifacts a ON a.id=json_extract(e.payload,'$.model_visible_result_ref')
             JOIN artifact_objects o ON o.hash=a.object_hash WHERE e.owner=?1 AND e.kind='tool.call_completed'"
        )?;
        let mut latest_active = true;
        let mut candidates = Vec::new();
        while let Some(row) = events.next()? {
            let sequence: i64 = row.get(0)?;
            let raw: String = row.get(1)?;
            let id = raw.parse().map_err(|_| Error::Conflict)?;
            if plan.omits(id) {
                continue;
            }
            let active: bool = row.get(3)?;
            let protected = required.contains(&id) || active && latest_active;
            if active {
                latest_active = false;
            }
            if !protected && plan.adopted.as_ref().is_some_and(|a| a.was_deferred(id, sequence)) {
                plan.defer(id, sequence);
                continue;
            }
            let retained = plan.adopted.as_ref().is_some_and(|a| a.events.contains(&id));
            candidates.push((
                sequence,
                id,
                raw,
                row.get::<_, Option<String>>(2)?,
                protected,
                retained,
            ));
        }
        // Spend the read allowance on mandatory groups, then the last adopted prefix. A new
        // optional group cannot silently displace a group from that prefix at the soft limit.
        candidates.sort_by_key(|(sequence, _, _, _, required, retained)| {
            Reverse((*required, *retained, *sequence))
        });
        for (sequence, id, raw, batch, protected, retained) in candidates {
            let bytes = if protected || (!plan.recent_boundary && remaining > 0) {
                let body: i64 = event_size.query_row([&raw], |r| r.get(0))?;
                let mut bytes = u64::try_from(body).map_err(|_| Error::Conflict)?;
                if let Some(batch) = batch {
                    let mut rows = result_sizes.query([batch])?;
                    while let Some(row) = rows.next()? {
                        let event: String = row.get(0)?;
                        let stored = row.get::<_, i64>(1)?;
                        let projected =
                            plan.adopted.as_ref().and_then(|prior| prior.results.get(&event));
                        let length = projected.map_or(stored, |value| value.bytes);
                        bytes = bytes
                            .saturating_add(u64::try_from(length).map_err(|_| Error::Conflict)?);
                    }
                }
                bytes
            } else {
                u64::MAX
            };
            if protected || (!plan.recent_boundary && bytes <= remaining) {
                remaining = remaining.saturating_sub(bytes);
                plan.selected_bytes = plan.selected_bytes.saturating_add(bytes);
            } else {
                plan.defer(id, sequence);
                plan.recent_boundary = true;
                if retained {
                    plan.missing_adopted.insert(id);
                }
                plan.compaction_pressure |= plan.adopted.is_some();
            }
        }
        plan.compaction_pressure |= plan.adopted.is_some() && plan.selected_bytes >= soft_limit;
        plan.remaining_bytes = Some(remaining);
        Ok(plan)
    }

    pub(crate) fn include_carried(&mut self, bytes: u64, events: &[(EventId, i64)]) -> bool {
        for &(id, sequence) in events {
            if self.adopted.as_ref().is_some_and(|a| a.was_deferred(id, sequence)) {
                self.defer(id, sequence);
            }
        }
        if events.iter().all(|(id, _)| self.omits(*id)) {
            return false;
        }
        if self.recent_boundary || self.remaining_bytes.is_some_and(|left| bytes > left) {
            for &(id, sequence) in events {
                if !self.omits(id) && self.adopted.as_ref().is_some_and(|a| a.events.contains(&id))
                {
                    self.missing_adopted.insert(id);
                }
                self.defer(id, sequence);
            }
            self.recent_boundary = true;
            self.compaction_pressure |= self.adopted.is_some();
            return false;
        }
        if let Some(remaining) = &mut self.remaining_bytes {
            *remaining = remaining.saturating_sub(bytes);
        }
        self.selected_bytes = self.selected_bytes.saturating_add(bytes);
        true
    }

    fn defer(&mut self, id: EventId, sequence: i64) {
        if self.skip.insert(id) {
            self.deferred += 1;
            self.deferred_range = Some(
                self.deferred_range
                    .map_or((sequence, sequence), |(a, b)| (a.min(sequence), b.max(sequence))),
            );
        }
    }
}
