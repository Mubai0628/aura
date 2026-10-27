//! Frozen query projections, field visibility, relevance and bounded summaries.
//! Publication ownership and bounded index mutation stay in the parent module.
use super::ActivityResourceKey;
use aura_activity_domain::*;
use aura_domain_common::{Revision, RuntimeInstanceId, SessionId, UtcTimestamp, WorkspaceId};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Eq, PartialEq)]
pub struct ActivitySnapshot {
    pub header: ActivitySnapshotHeader,
    pub rows: Vec<ActivityRow>,
    // Bounded, frozen references for relevance and privacy projection. These are never wire
    // fields; redacting a name must not erase the original resource relationship.
    pub(super) captured: Vec<Arc<ActivityRecord>>,
}

impl std::fmt::Debug for ActivitySnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActivitySnapshot")
            .field("header", &self.header)
            .field("rows", &self.rows)
            .finish_non_exhaustive()
    }
}

impl ActivitySnapshot {
    pub(super) fn unavailable(view: &ActivityVisibility, sampled_at: UtcTimestamp) -> Self {
        Self {
            header: ActivitySnapshotHeader {
                sampled_at,
                runtime_instance_id: view.runtime_instance_id,
                revision: Revision::INITIAL,
                coverage: ActivityCoverage::unavailable(),
                stale: false,
            },
            rows: Vec::new(),
            captured: Vec::new(),
        }
    }
}

impl ActivitySnapshot {
    /// Answers a precise query over **this held snapshot**.
    ///
    /// Paging is a property of a snapshot, not of the live index: two pages of one
    /// query are two calls on the *same* held value, which is what makes "the page
    /// is consistent" a checkable fact rather than a timing hope.
    #[must_use]
    pub fn query(&self, query: ActivityQuery, offset: u32, limit: u32) -> ActivityQueryResult {
        let limit = limit.clamp(1, MAX_ACTIVITY_PAGE_ROWS as u32);
        let matching = self.rows.iter().filter(|row| matches_query(row, &query));
        let total = matching.clone().count();
        let rows = matching.skip(offset as usize).take(limit as usize).cloned().collect::<Vec<_>>();
        let mut header = self.header.clone();
        if let ActivityQuery::Resource { target } = &query
            && (matches!(target, ActivityResourceTarget::Unproven { .. })
                || !target.is_within_request_bounds())
        {
            header.coverage.state = ActivityCoverageState::Unavailable;
            if !header.coverage.omission_reasons.contains(&ActivityOmissionReason::IndexUnavailable)
            {
                header.coverage.omission_reasons.push(ActivityOmissionReason::IndexUnavailable);
            }
        }
        header.coverage.omitted_by_limit =
            u64::try_from(total.saturating_sub(rows.len())).unwrap_or(0);
        if header.coverage.omitted_by_limit > 0 {
            if header.coverage.state != ActivityCoverageState::Unavailable {
                header.coverage.state = ActivityCoverageState::Partial;
            }
            header.coverage.truncated = true;
            if !header.coverage.omission_reasons.contains(&ActivityOmissionReason::SummaryRowLimit)
            {
                header.coverage.omission_reasons.push(ActivityOmissionReason::SummaryRowLimit);
            }
        }
        ActivityQueryResult {
            header,
            query,
            offset,
            limit,
            has_more: (offset as usize + rows.len()) < total,
            rows,
        }
    }

    /// Re-reads this same held snapshot under a different visibility.
    ///
    /// It re-projects the *rows already sampled*, so the sampling instant and the
    /// revision are the ones the caller is holding. It never goes back to the
    /// index, which is what keeps two readers of one step on one picture.
    #[must_use]
    pub fn reproject(&self, view: &ActivityVisibility) -> Self {
        let mut projected = self.clone();
        projected.captured.retain(|record| view.scope_matches(record) && view.may_see(record));
        projected.rows = projected.captured.iter().map(|record| view.project(record)).collect();
        projected
    }

    /// Selects the relevant summary within a fixed budget.
    ///
    /// Relevance is decided *after* the visibility filter that produced these
    /// rows, and never by comparing workspace ids: a Shell whose write set is not
    /// provable stays relevant even when it belongs to another workspace, because
    /// "another workspace" is not evidence that it cannot touch this target.
    #[must_use]
    pub fn select_summary(
        &self,
        view: &ActivityVisibility,
        budget: ActivitySummaryBudget,
    ) -> ActivitySummary {
        let mut selected = Vec::new();
        let mut omitted = 0u64;
        let mut bytes = 0usize;
        let row_budget = budget.max_bytes.saturating_sub(320);
        let mut ordered = self
            .captured
            .iter()
            .filter(|record| view.scope_matches(record) && view.may_see(record))
            .map(|record| (view.project(record), view.record_relevance(record)))
            .collect::<Vec<_>>();
        ordered.sort_by_key(|(row, _)| (row.session_id != view.session_id, row.started_at, row.id));
        let mut byte_limited = false;
        for (row, relevance) in ordered {
            if selected.len() >= budget.max_rows.min(DEFAULT_ACTIVITY_SUMMARY_ROWS) {
                omitted += 1;
                continue;
            }
            if relevance == ActivityRelevance::Irrelevant {
                omitted += 1;
                continue;
            }
            let rendered = render_row(&row, relevance, view.may_see_private_fields(&row));
            if bytes
                .saturating_add(rendered.len())
                .saturating_add(usize::from(!selected.is_empty()))
                > row_budget
            {
                byte_limited = true;
                omitted += 1;
                continue;
            }
            bytes += rendered.len() + usize::from(!selected.is_empty());
            selected.push(ActivitySummaryRow { row, relevance, rendered });
        }
        let mut header = self.header.clone();
        if omitted > 0 {
            header.coverage.state = if header.coverage.state == ActivityCoverageState::Unavailable {
                ActivityCoverageState::Unavailable
            } else {
                ActivityCoverageState::Partial
            };
            header.coverage.omitted_by_limit = omitted;
            header.coverage.truncated = true;
            let reason = if byte_limited {
                ActivityOmissionReason::SummaryByteLimit
            } else {
                ActivityOmissionReason::SummaryRowLimit
            };
            if !header.coverage.omission_reasons.contains(&reason) {
                header.coverage.omission_reasons.push(reason);
            }
        }
        let body = selected.iter().map(|row| row.rendered.as_str()).collect::<Vec<_>>().join("\n");
        ActivitySummary { header, rows: selected, body }
    }
}

/// One selected summary row with the relevance that admitted it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivitySummaryRow {
    pub row: ActivityRow,
    pub relevance: ActivityRelevance,
    pub rendered: String,
}

/// The frozen summary. Its `body` is what becomes the activity fragment; its
/// `header` is what the Inspector and the coverage report show.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivitySummary {
    pub header: ActivitySnapshotHeader,
    pub rows: Vec<ActivitySummaryRow>,
    pub body: String,
}

impl ActivitySummary {
    /// An empty summary produced by an index that could not answer.
    #[must_use]
    pub fn unavailable(view: &ActivityVisibility) -> Self {
        Self {
            header: ActivitySnapshot::unavailable(view, UtcTimestamp::now()).header,
            rows: Vec::new(),
            body: String::new(),
        }
    }

    /// Whether this summary may be sent to a model at all.
    ///
    /// The rule is about whether the index **answered**, not about whether anything was
    /// running. A healthy index that found nothing is sendable, and it must be: "the index
    /// answered and no writer is registered" is a real, useful fact, and suppressing it would
    /// leave the model unable to tell an idle runtime from a blind one - exactly the confusion
    /// the design forbids. An unavailable index is not sendable, because the only honest thing
    /// it could say is that it does not know.
    ///
    /// The body itself may legitimately be empty; the fragment always carries its header,
    /// which is where the coverage fact lives.
    #[must_use]
    pub fn is_sendable(&self) -> bool {
        self.header.coverage.state != ActivityCoverageState::Unavailable
    }
}

/// The fixed budget for one summary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivitySummaryBudget {
    pub max_rows: usize,
    pub max_bytes: usize,
}

impl Default for ActivitySummaryBudget {
    fn default() -> Self {
        Self { max_rows: DEFAULT_ACTIVITY_SUMMARY_ROWS, max_bytes: MAX_ACTIVITY_SUMMARY_BYTES }
    }
}

/// How relevant one row is to the reader.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivityRelevance {
    /// Own session, or the target this reader already named.
    Direct,
    /// Reachable through a resource this reader may touch, in this reader's workspace, or an
    /// unknown write set outside its own workspace.
    Related,
    /// Not selected.
    Irrelevant,
}

/// The real visibility of one reader.
///
/// This is a *projection input*, not a second authorisation. It is derived from
/// the permission snapshot and the session the reader really belongs to; it
/// cannot widen what the model-egress authorizer later allows.
#[derive(Clone, Debug)]
pub struct ActivityVisibility {
    pub runtime_instance_id: RuntimeInstanceId,
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    /// Physical targets this reader already named. A row touching one of these is
    /// directly relevant even from another session or another workspace.
    pub relevant_targets: BTreeSet<ActivityResourceKey>,
    /// When false, another session's rows are counted but not shown.
    pub cross_session_visible: bool,
    pub authorized_targets: BTreeSet<ActivityResourceKey>,
    /// Complete known targets for directory containment; lookup hints remain non-authorizing.
    pub relevant_resources: Vec<ActivityResourceTarget>,
    /// This reader's own spelling of targets it already named. Another session's row touching one
    /// carries this name, never its owner's.
    pub relevant_names: BTreeMap<ActivityResourceKey, String>,
}

impl ActivityVisibility {
    #[must_use]
    pub fn for_session(
        runtime_instance_id: RuntimeInstanceId,
        session_id: SessionId,
        workspace_id: WorkspaceId,
    ) -> Self {
        Self {
            runtime_instance_id,
            session_id,
            workspace_id,
            relevant_targets: BTreeSet::new(),
            cross_session_visible: false,
            authorized_targets: BTreeSet::new(),
            relevant_resources: Vec::new(),
            relevant_names: BTreeMap::new(),
        }
    }

    pub(super) fn project(&self, record: &ActivityRecord) -> ActivityRow {
        let own = record.owner.session_id == self.session_id;
        let resources = record
            .resources
            .iter()
            .filter_map(|resource| {
                if own {
                    return Some(resource.clone());
                }
                if self.is_related(&resource.target) {
                    let mut projected = resource.clone();
                    // A physical key is not authority over another session's spelling of that file.
                    // The name shown is the reader's own, supplied by its own task.
                    projected.display_name = self
                        .relevant_names
                        .get(&ActivityResourceKey::of(&resource.target))
                        .cloned()
                        .unwrap_or_default();
                    projected.display_name_truncated = false;
                    // Names not already carried by the requesting session remain private,
                    // including prospective child names discovered through a directory hint.
                    if matches!(
                        projected.target,
                        ActivityResourceTarget::VerifiedEntry { .. }
                            | ActivityResourceTarget::DirectoryEntry { .. }
                    ) {
                        projected.target = self.reader_entry(&resource.target);
                    }
                    Some(projected)
                } else if resource.relation.is_unknown_write_set() {
                    // The folder a command runs in is metadata of its own workspace, shown to a
                    // reader there; elsewhere only the unprovable writer itself is reported.
                    let folder = record.owner.workspace_id == self.workspace_id
                        && resource.relation == ActivityResourceRelation::UnknownWriteSet;
                    Some(ActivityResource {
                        target: ActivityResourceTarget::Unproven { hint: String::new() },
                        relation: resource.relation,
                        display_name: if folder {
                            resource.display_name.clone()
                        } else {
                            String::new()
                        },
                        display_name_truncated: folder && resource.display_name_truncated,
                    })
                } else {
                    None
                }
            })
            .collect();
        ActivityRow {
            id: record.id,
            kind: record.kind,
            tool_id: record.tool_id,
            tool_name: record.tool_name.clone(),
            private_source: record.private_source,
            tool_group: record.tool_group,
            session_id: record.owner.session_id,
            workspace_id: record.owner.workspace_id,
            agent_run_id: record.owner.agent_run_id,
            execution: record.execution.clone(),
            resources,
            parameters: if own { record.parameters.clone() } else { Vec::new() },
            started_at: record.started_at,
        }
    }

    fn reader_entry(&self, target: &ActivityResourceTarget) -> ActivityResourceTarget {
        let key = ActivityResourceKey::of(target);
        if let Some(known) =
            self.relevant_resources.iter().find(|known| ActivityResourceKey::of(known) == key)
        {
            return known.clone();
        }
        if self.relevant_targets.contains(&key) {
            // Only return the canonical key already supplied by the reader. Never copy
            // another session's original spelling, even for an exact equivalent name.
            match (target, key) {
                (
                    ActivityResourceTarget::VerifiedEntry { parent, ancestors, .. },
                    ActivityResourceKey::VerifiedEntry { pending, folded, .. },
                ) => {
                    return ActivityResourceTarget::VerifiedEntry {
                        parent: parent.clone(),
                        ancestors: ancestors.clone(),
                        comparison_keys: Some(pending.clone()),
                        pending,
                        case_insensitive: folded,
                    };
                }
                (
                    ActivityResourceTarget::DirectoryEntry {
                        parent_volume_serial,
                        parent_file_index,
                        ..
                    },
                    ActivityResourceKey::Entry { leaf, folded, .. },
                ) => {
                    return ActivityResourceTarget::DirectoryEntry {
                        parent_volume_serial: *parent_volume_serial,
                        parent_file_index: *parent_file_index,
                        comparison_key: Some(leaf.clone()),
                        leaf,
                        case_insensitive: folded,
                    };
                }
                _ => {}
            }
        }
        ActivityResourceTarget::Unproven { hint: String::new() }
    }

    fn record_relevance(&self, record: &ActivityRecord) -> ActivityRelevance {
        if record.owner.session_id == self.session_id {
            ActivityRelevance::Direct
        } else if record.owner.workspace_id == self.workspace_id
            || self.names_a_relevant_target(record)
            || Self::writes_outside_unknown(record)
        {
            ActivityRelevance::Related
        } else {
            ActivityRelevance::Irrelevant
        }
    }
    /// Whether a row is in this reader's scope at all. Scope is deliberately
    /// wider than visibility: a same-physical-target row from another workspace
    /// is *in scope*, so it can be reported as filtered rather than pretended away.
    ///
    /// An unknown write set is in scope in its own workspace. A Shell runs in its session's
    /// workspace, so a writer whose set cannot be proven shares this reader's files only when it
    /// shares this reader's workspace - unless its script names places outside its workspace, which
    /// puts it in every reader's scope. Any other command in another workspace reaches this reader
    /// only through a physical target the reader already reached, like any other row there.
    #[must_use]
    pub fn scope_matches(&self, record: &ActivityRecord) -> bool {
        if record.runtime_instance_id != self.runtime_instance_id {
            return false;
        }
        record.owner.session_id == self.session_id
            || record.owner.workspace_id == self.workspace_id
            || record.resources.iter().any(|resource| self.is_related(&resource.target))
            || Self::writes_outside_unknown(record)
    }

    /// Whether the named physical target is one this reader already reached (R4).
    fn is_related(&self, target: &ActivityResourceTarget) -> bool {
        !matches!(target, ActivityResourceTarget::Unproven { .. })
            && (self.relevant_targets.contains(&ActivityResourceKey::of(target))
                || self
                    .relevant_resources
                    .iter()
                    .take(aura_activity_domain::MAX_ACTIVITY_RELEVANT_HINTS)
                    .any(|known| targets_related(known, target)))
    }

    #[must_use]
    fn names_a_relevant_target(&self, record: &ActivityRecord) -> bool {
        record.resources.iter().any(|resource| self.is_related(&resource.target))
    }

    /// Whether this row's write set cannot be proven (R4).
    #[must_use]
    fn is_unknown_write_set(record: &ActivityRecord) -> bool {
        record.resources.iter().any(|resource| resource.relation.is_unknown_write_set())
    }

    /// Whether this row's unprovable write set reaches outside its own workspace.
    #[must_use]
    fn writes_outside_unknown(record: &ActivityRecord) -> bool {
        record.resources.iter().any(|resource| {
            resource.relation == ActivityResourceRelation::UnknownWriteSetOutsideWorkspace
        })
    }

    /// The visibility decision, applied *before* relevance and budget.
    ///
    /// R4: VISIBILITY IS NOT A PROPERTY OF THE QUESTION.
    /// `cross_session_visible` is a caller-supplied widening, and the previous shape let the
    /// *broadness of a query* set it - so merely asking a precise question admitted every other
    /// session's private cwd, path and parameter values into the model's context. A query type is
    /// not an authorisation, and neither is a target named in a request: those are lookup hints.
    ///
    /// So another session's row is visible in exactly two cases, and both are about the reader's
    /// own legitimate position rather than about how it phrased the question:
    ///
    /// * it names a physical target this reader already reached - the reader has an independent
    ///   reason to know that object exists, and
    /// * its write set cannot be proven and it runs in this reader's workspace or writes outside its
    ///   own, in which case only the **minimum permitted** fact is admitted: that an unprovable
    ///   writer exists, never its private parameters. Another workspace's unprovable writer that
    ///   stays inside its workspace is not shown.
    ///
    /// Every other other-session row is counted as filtered rather than shown, which is what makes
    /// "there is something here you may not see" a real, disclosed fact instead of a silent gap.
    #[must_use]
    pub fn may_see(&self, record: &ActivityRecord) -> bool {
        if record.owner.session_id == self.session_id {
            return true;
        }
        // `cross_session_visible` is a caller-supplied widening, and it is deliberately **not**
        // sufficient on its own: a caller that asks a broad question still only reaches another
        // session's row when that row names a physical target the caller already reached, so the
        // widening cannot be manufactured by the shape of a query. An unprovable write set in this
        // reader's workspace is the one case admitted without such an association, because the
        // row's mere existence is the fact that matters - and `may_see_private_fields` still keeps
        // its private content out.
        self.names_a_relevant_target(record)
            || record.owner.workspace_id == self.workspace_id && Self::is_unknown_write_set(record)
            || Self::writes_outside_unknown(record)
    }

    /// Whether this row may contribute its **private** fields - display names, paths and
    /// parameters - to a reader that does not own it (R4).
    ///
    /// This is the field-source half of the decision, and it is deliberately separate from
    /// [`Self::may_see`]. Knowing that *some* unprovable writer exists is a different fact from
    /// knowing which file another session has open or what arguments it passed, and the second one
    /// needs a real association - a physical target this reader already named - not merely the
    /// existence of the row. Core-generated metadata does not make another session's paths public.
    #[must_use]
    pub fn may_see_private_fields(&self, row: &ActivityRow) -> bool {
        row.session_id == self.session_id
    }

    /// Relevance of one already-visible row.
    #[must_use]
    pub fn relevance(&self, row: &ActivityRow) -> ActivityRelevance {
        if row.session_id == self.session_id {
            return ActivityRelevance::Direct;
        }
        if row.workspace_id == self.workspace_id {
            return ActivityRelevance::Related;
        }
        // A row from another workspace stays relevant when it touches a target this reader
        // named, or writes outside its workspace: hiding it would present an unrelated-looking
        // picture of the same physical file.
        if row.resources.iter().any(|resource| {
            resource.relation == ActivityResourceRelation::UnknownWriteSetOutsideWorkspace
                || self.is_related(&resource.target)
        }) {
            return ActivityRelevance::Related;
        }
        ActivityRelevance::Irrelevant
    }
}

/// Renders one summary row (R4).
///
/// `private` says whether this reader may see the row's **own** display names and parameters. When
/// it may not, the row is still reported - the reader really does need to know that an unprovable
/// writer exists - but only the minimum permitted facts survive: the kind, the tool group, the
/// relation and the time. A path or an argument belonging to another session is never rendered,
/// because Core-generated metadata about that row does not make its content public.
fn render_row(row: &ActivityRow, relevance: ActivityRelevance, private: bool) -> String {
    let relation = row
        .resources
        .iter()
        .map(|resource| match resource.relation {
            ActivityResourceRelation::Read => "read",
            ActivityResourceRelation::Write => "write",
            ActivityResourceRelation::UnknownWriteSet => "unknown-write-set",
            ActivityResourceRelation::UnknownWriteSetOutsideWorkspace => {
                "unknown-write-set-outside-workspace"
            }
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("+");
    let target = if private {
        row.resources
            .iter()
            .map(|resource| resource.display_name.as_str())
            .collect::<Vec<_>>()
            .join(",")
    } else {
        // The minimum permitted fact. It states that the target is not disclosed here, which is
        // itself a truthful report rather than a silently empty field.
        String::new()
    };
    let relevance = match relevance {
        ActivityRelevance::Direct => "direct",
        ActivityRelevance::Related => "related",
        ActivityRelevance::Irrelevant => "irrelevant",
    };
    let target = if target.is_empty() {
        if private { "(no proven target)".to_owned() } else { "(target not disclosed)".to_owned() }
    } else {
        target
    };
    format!(
        "- id={} tool={} {} {} [{}] target={} params={} started={}{}",
        serde_json::to_string(&row.id).unwrap_or_default(),
        if row.tool_name.is_empty() { row.tool_id.to_string() } else { row.tool_name.clone() },
        row.kind.as_str(),
        row.tool_group.as_str(),
        relation,
        serde_json::to_string(&target).unwrap_or_default(),
        serde_json::to_string(&if private { row.parameters.clone() } else { Vec::new() })
            .unwrap_or_default(),
        row.started_at,
        if relevance == "direct" { String::new() } else { format!(" ({relevance})") }
    )
}

#[must_use]
pub fn targets_related(left: &ActivityResourceTarget, right: &ActivityResourceTarget) -> bool {
    use ActivityResourceTarget::*;
    if matches!(left, Unproven { .. }) || matches!(right, Unproven { .. }) {
        return false;
    }
    let left_key = ActivityResourceKey::of(left);
    let right_key = ActivityResourceKey::of(right);
    if matches!(left_key, ActivityResourceKey::Unproven { .. })
        || matches!(right_key, ActivityResourceKey::Unproven { .. })
    {
        return false;
    }
    if left_key == right_key {
        return true;
    }
    fn within(directory: &ActivityResourceTarget, child: &ActivityResourceTarget) -> bool {
        if let VerifiedObject { identity, subtree: true, .. } = directory {
            return match child {
                VerifiedObject { identity: child, ancestors, .. } => {
                    identity == child || ancestors.contains(identity)
                }
                VerifiedEntry { parent, ancestors, .. } => {
                    identity == parent || ancestors.contains(identity)
                }
                _ => false,
            };
        }
        if let VerifiedEntry { parent, case_insensitive, .. } = directory
            && let VerifiedEntry { parent: other, case_insensitive: other_case, .. } = child
            && let (Some(pending), Some(tail)) = (directory.name_keys(), child.name_keys())
        {
            return parent == other
                && case_insensitive == other_case
                && pending.len() < tail.len()
                && tail.starts_with(&pending);
        }
        false
    }
    within(left, right) || within(right, left)
}

fn matches_query(row: &ActivityRow, query: &ActivityQuery) -> bool {
    match query {
        ActivityQuery::Session => true,
        ActivityQuery::Exact { id } => row.id == *id,
        ActivityQuery::Group { group } => row.tool_group == *group,
        ActivityQuery::UnknownWriteSet => {
            row.resources.iter().any(|resource| resource.relation.is_unknown_write_set())
        }
        ActivityQuery::Resource { target } => {
            row.resources.iter().any(|resource| targets_related(&resource.target, target))
        }
    }
}
