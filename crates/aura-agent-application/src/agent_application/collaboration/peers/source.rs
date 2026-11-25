use super::*;
use aura_agent_collaboration::{
    PeerSourceCatalogIssue as Issue, PeerSourceCursor, PeerSourceEntry,
};
use aura_agent_domain::{
    SourceMembership, TaskLineageRelationKind, TaskLineageSource, UserMessageTextRange,
};
use aura_persistence_v2::agent::{attachments, sender};
use std::ops::Range;

const PAGE_SIZE: usize = 64;
const WITHDRAWN: &str = "peers.source_withdrawn: 原用户来源已撤回或不可核验；没有读取。";
const CHANGED: &str =
    "peers.source_changed: 来源内容已更正或撤回；请重新读取投递，旧片段不再授权。";

pub(super) fn visible(
    delivery: &dispatch::Delivery,
    session: SessionId,
    workspace: WorkspaceId,
) -> bool {
    delivery.origin.workspace_id == workspace
        && (delivery.sender.session_id == session || delivery.target == session)
}

pub(super) fn lookup(
    db: &Connection,
    caller: SessionId,
    workspace: WorkspaceId,
    input: PeerLookup,
    view_for: impl Fn(&dispatch::Delivery) -> StoreResult<PeerDeliveryView>,
) -> StoreResult<PeerLookupResult> {
    check_caller(db, caller, workspace)?;
    let ids = match (input.delivery_id, input.tool_call_id, input.source_cursor) {
        (Some(id), None, _) => vec![id],
        (None, Some(call), None) => dispatch::by_call(db, caller, call)?,
        _ => {
            return Err(Error::Invalid(
                "peers.lookup_invalid: 请提供投递 ID 或原工具调用 ID；来源游标只能与投递 ID 一起使用。",
            ));
        }
    };
    let mut deliveries = Vec::new();
    for id in ids {
        let Some(delivery) = dispatch::load(db, id)? else { continue };
        if !visible(&delivery, caller, workspace) {
            continue;
        }
        let mut view = view_for(&delivery)?;
        if input.delivery_id.is_some() {
            // Even a missing or invalid source cannot hide a durable status/result receipt.
            match catalog(db, &delivery, input.source_cursor).unwrap_or(Err(Issue::Unavailable)) {
                Ok((sources, next)) => {
                    view.sources = sources;
                    view.next_source_cursor = next;
                }
                Err(issue) => view.source_catalog_issue = Some(issue),
            }
        }
        deliveries.push(view);
    }
    Ok(PeerLookupResult { deliveries })
}

fn check_caller(db: &Connection, session: SessionId, workspace: WorkspaceId) -> StoreResult<()> {
    if teams::member_by_child(db, session)?.is_some()
        || aura_persistence_v2::workspace::session(db, session)?
            .is_none_or(|s| s.workspace_id != workspace)
    {
        return Err(Error::Conflict);
    }
    Ok(())
}

type Catalog = Result<(Vec<PeerSourceEntry>, Option<PeerSourceCursor>), Issue>;

fn catalog(
    db: &Connection,
    delivery: &dispatch::Delivery,
    cursor: Option<PeerSourceCursor>,
) -> StoreResult<Catalog> {
    let Some(current) = delivery_lineage(db, delivery)? else { return Ok(Err(Issue::Unavailable)) };
    if cursor.is_some_and(|c| {
        c.delivery_id != delivery.id
            || c.lineage_revision != current.revision
            || c.after_sequence == 0
            || c.after_sequence as usize >= current.sources.len()
    }) {
        return Ok(Err(Issue::RestartFromFirstPage));
    }
    // Source sequence is append-only in a lineage. The revision prevents pages from
    // straddling a correction, and the ID prevents reuse against another delivery.
    let after = cursor.map_or(0, |c| c.after_sequence);
    let mut entries: Vec<PeerSourceEntry> = Vec::new();
    for source in current.sources.iter().skip(after as usize) {
        if let Material::Text(text) = authorized(db, &current, source)? {
            if entries.len() == PAGE_SIZE {
                let next = PeerSourceCursor {
                    delivery_id: delivery.id,
                    lineage_revision: current.revision,
                    after_sequence: entries[PAGE_SIZE - 1].source_sequence,
                };
                return Ok(Ok((entries, Some(next))));
            }
            entries.push(PeerSourceEntry {
                event_id: source.user_event_id,
                source_sequence: source.source_sequence,
                content_hash: source.content_hash.clone(),
                total_chars: count(&text),
            });
        }
    }
    Ok(Ok((entries, None)))
}

fn delivery_lineage(
    db: &Connection,
    delivery: &dispatch::Delivery,
) -> StoreResult<Option<TaskLineage>> {
    if delivery.cancelled {
        return Ok(None);
    }
    origin::verified_lineage(db, &delivery.origin)
}

/// Reauthorizes every page; a previously returned event ID is not a grant.
pub(super) fn read(
    db: &Connection,
    caller: PeerSender,
    workspace: WorkspaceId,
    input: &PeerSourceInput,
) -> StoreResult<PeerSourceContent> {
    check_caller(db, caller.session_id, workspace)?;
    let delivery = dispatch::load(db, input.delivery_id)?.ok_or(Error::Conflict)?;
    if !visible(&delivery, caller.session_id, workspace) {
        return Err(Error::Conflict);
    }
    let current = delivery_lineage(db, &delivery)?.ok_or(Error::Invalid(WITHDRAWN))?;
    let source = current
        .sources
        .iter()
        .find(|s| s.user_event_id == input.event_id)
        .ok_or(Error::Invalid(CHANGED))?;
    let text = match authorized(db, &current, source)? {
        Material::Text(text) => text,
        Material::Absent => return Err(Error::Invalid(CHANGED)),
        Material::Sensitive => {
            return Err(Error::Invalid("peers.source_secret: 来源可能含有凭据；没有读取。"));
        }
    };
    let total_chars = count(&text);
    let offset = input.offset.min(total_chars);
    let max_chars = input
        .max_chars
        .unwrap_or(aura_agent_collaboration::MAX_PEER_SOURCE_CHARS)
        .clamp(1, aura_agent_collaboration::MAX_PEER_SOURCE_CHARS);
    let text: String = text.chars().skip(offset as usize).take(max_chars as usize).collect();
    let next = offset.saturating_add(count(&text));
    let complete = next >= total_chars;
    Ok(PeerSourceContent {
        delivery_id: input.delivery_id,
        event_id: input.event_id,
        content_hash: source.content_hash.clone(),
        offset,
        total_chars,
        text,
        next_offset: (!complete).then_some(next),
        complete,
    })
}

fn count(text: &str) -> u32 {
    u32::try_from(text.chars().count()).unwrap_or(u32::MAX)
}

pub(super) enum Material {
    Text(String),
    Absent,
    Sensitive,
}

pub(super) fn authorized(
    db: &Connection,
    lineage: &TaskLineage,
    source: &TaskLineageSource,
) -> StoreResult<Material> {
    if source.membership != SourceMembership::Active
        || sender::of_message(db, source.user_event_id)?.is_some()
    {
        return Ok(Material::Absent);
    }
    let mut withdrawn = Vec::new();
    let mut classification = None;
    for relation in &lineage.relations {
        if relation.earlier_user_event_id == Some(source.user_event_id)
            && matches!(
                relation.kind,
                TaskLineageRelationKind::Corrects | TaskLineageRelationKind::CancelsPart
            )
        {
            let Some(range) = &relation.earlier_excerpt else { return Ok(Material::Absent) };
            withdrawn.push(byte_range(range)?);
        } else if relation.later_user_event_id == source.user_event_id
            && relation.kind == TaskLineageRelationKind::ReclassifiesUnrelatedPart
        {
            let Some(range) = &relation.later_excerpt else { return Ok(Material::Absent) };
            withdrawn.push(byte_range(range)?);
            classification.get_or_insert((relation.created_by_turn_task_id, relation.created_at));
        }
    }
    withdrawn.sort_unstable_by_key(|r| (r.start, r.end));
    let message = session::user_message(db, lineage.session_id, source.user_event_id)?;
    if message.content_hash != source.content_hash {
        return Err(Error::Conflict);
    }
    let typed = attachments::of_message(db, source.user_event_id)?
        .map_or(Ok(message.text.len()), |(n, _)| usize::try_from(n).map_err(|_| Error::Conflict))?;
    let text = message.text.get(..typed).ok_or(Error::Conflict)?;
    // Check before taking excerpts so a range that bisects a credential cannot reveal it.
    if aura_secret_scanner::detect_text(text).is_some() {
        return Ok(Material::Sensitive);
    }
    let mut permitted = if source.related_ranges.is_empty() {
        std::iter::once(0..typed).collect()
    } else {
        source.related_ranges.iter().map(byte_range).collect::<StoreResult<Vec<_>>>()?
    };
    if let Some(decision) = classification {
        // An in-flight addition can already be attached by the time a mixed
        // decision records its scope. Keep the original attachment immutable;
        // intersect it with the explicit related excerpts in that decision.
        // Later reclassifications also remove their unrelated ranges above.
        let mut retained = lineage
            .relations
            .iter()
            .filter(|r| {
                r.later_user_event_id == source.user_event_id
                    && (r.created_by_turn_task_id, r.created_at) == decision
                    && r.kind != TaskLineageRelationKind::ReclassifiesUnrelatedPart
            })
            .filter_map(|r| r.later_excerpt.as_ref())
            .map(byte_range)
            .collect::<StoreResult<Vec<_>>>()?;
        retained.sort_unstable_by_key(|r| (r.start, r.end));
        retained.dedup();
        if retained.windows(2).any(|pair| pair[0].end > pair[1].start) {
            return Err(Error::Conflict);
        }
        permitted = retained
            .into_iter()
            .flat_map(|r| {
                permitted.iter().filter_map(move |original| {
                    let start = original.start.max(r.start);
                    let end = original.end.min(r.end);
                    (start < end).then_some(start..end)
                })
            })
            .collect();
    }
    let mut fragments = Vec::new();
    for range in permitted {
        text.get(range.clone()).ok_or(Error::Conflict)?;
        let mut from = range.start;
        for cut in &withdrawn {
            if cut.start >= range.end {
                break;
            }
            if cut.end <= from {
                continue;
            }
            if cut.start > from {
                fragments.push(text.get(from..cut.start).ok_or(Error::Conflict)?);
            }
            from = cut.end.min(range.end);
        }
        if from < range.end {
            fragments.push(text.get(from..range.end).ok_or(Error::Conflict)?);
        }
    }
    if fragments.is_empty() {
        return Ok(Material::Absent);
    }
    let text = fragments.join("\n");
    if aura_secret_scanner::detect_text(&text).is_some() {
        return Ok(Material::Sensitive);
    }
    Ok(Material::Text(text))
}

fn byte_range(range: &UserMessageTextRange) -> StoreResult<Range<usize>> {
    let start = usize::try_from(range.start_utf8_byte).map_err(|_| Error::Conflict)?;
    let end = usize::try_from(range.end_utf8_byte).map_err(|_| Error::Conflict)?;
    if start >= end {
        return Err(Error::Conflict);
    }
    Ok(start..end)
}
