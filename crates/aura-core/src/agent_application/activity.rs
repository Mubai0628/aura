//! The activity fragment of `CurrentTaskState`.
//!
//! WHERE THIS LIVES, AND WHY
//! The fragment belongs to the `CurrentTaskState` section, beside the existing
//! `agent.control_state.v1` fragment, and it is a **separate** fragment rather than a field
//! inside the control state. The ordinary control-state refresh in
//! `agent_application::context` rewrites exactly one fragment - the one whose
//! `ContextSourceRef::RuntimeState.state_kind` is `agent.control_state.v1` - so a fragment
//! that is not that one survives the refresh by construction rather than by agreement. The
//! directed test `a_control_state_refresh_does_not_overwrite_the_activity_fragment` drives
//! the real refresh path to prove it.
//!
//! There is no thirteenth section and no new user message: `ContextSectionKind` still has
//! twelve members, and `assemble_context_sections` already supports several fragments per
//! section.
//!
//! WHY THE SNAPSHOT IS FROZEN HERE AND NOT IN THE ADAPTER
//! The adapter is called once per transport attempt. If it sampled the index, a retry would
//! send different bytes than the attempt it is retrying, and the "same frozen context" the
//! request snapshot claims to describe would be false. So the sampling happens exactly once,
//! when the model step's context is prepared, and the compiled bytes are what every retry
//! re-sends.
//!
//! WHY AN UNAVAILABLE INDEX IS NOT AN ERROR
//! A model step that cannot read the activity index still has a perfectly legitimate
//! execution to perform. An unavailable index therefore produces **no fragment at all**
//! rather than a failed preparation, and the coverage fact is carried by the Inspector
//! projection instead of being smuggled into the request body.
use super::*;
use aura_activity_domain::ActivityCoverageState;
use aura_activity_runtime::ActivityVisibility;
use aura_activity_runtime::{ActivitySummary, ActivitySummaryBudget};
use aura_context_domain::{ContextSourceRef, ModelEgressAuthorizationBasis, ModelEgressSourceKind};
use aura_context_runtime::ContextFragment;
use aura_context_runtime::PromptInspectorInput;
use aura_security_contract::NetworkDataClass;

/// The intact query result enters ordinary tool history. It must retain the class
/// of its private fields rather than acquiring the generic WorkspaceSource label.
/// Mixed results use PersonalData for the whole envelope; result bytes, hashes and
/// tool history grouping stay intact. Failure envelopes retain the existing class.
pub(super) fn query_history_class(content: &str) -> anyhow::Result<NetworkDataClass> {
    use aura_activity_domain::{
        ACTIVITY_QUERY_CONTRACT_VERSION, ActivityPrivateSource, AgentActivityQueryResultV1,
    };
    use aura_tool_contract::{ModelVisibleToolResultOutcomeV1, ModelVisibleToolSuccessPayloadV1};
    // The history holds the model's view of the result (`model_view`), not the whole
    // envelope; both carry the outcome, which is all this reads.
    let mut content: serde_json::Value = serde_json::from_str(content)?;
    let outcome: ModelVisibleToolResultOutcomeV1 =
        serde_json::from_value(content["outcome"].take())?;
    let ModelVisibleToolResultOutcomeV1::Succeeded { payload } = outcome else {
        return Ok(NetworkDataClass::WorkspaceSource);
    };
    let ModelVisibleToolSuccessPayloadV1::StructuredJson { value } = payload else {
        anyhow::bail!("agent.activity_query_payload_invalid");
    };
    let result: AgentActivityQueryResultV1 = serde_json::from_value(value)?;
    anyhow::ensure!(
        result.contract_version == ACTIVITY_QUERY_CONTRACT_VERSION,
        "agent.activity_query_version_invalid"
    );
    Ok(
        if result
            .page
            .rows
            .iter()
            .any(|row| row.private_source == ActivityPrivateSource::PersonalData)
        {
            NetworkDataClass::PersonalData
        } else {
            NetworkDataClass::WorkspaceMetadata
        },
    )
}

/// The frozen activity projection for one model step.
///
/// It is an ordinary value, kept for the duration of one `prepare` call, so every reader on
/// that path - the fragment, the Inspector projection, the summary and the coverage report -
/// describes exactly one sampling instant.
pub(super) struct FrozenActivity {
    pub summary: ActivitySummary,
    /// The model step this snapshot was frozen for.
    ///
    /// It is carried so that "one snapshot per step" is a checkable fact rather than an
    /// intention: a reader holding two frozen values can tell whether they are one step's
    /// picture or two different steps'.
    pub model_step_id: ModelStepId,
    /// The revision the index was at when it was sampled, so a later reader can tell whether
    /// the picture is still current.
    pub revision: aura_domain_common::Revision,
}

struct RenderedActivity {
    fragments: Vec<ContextFragment>,
    shown: u32,
    extra_omitted: u64,
    /// Which session runs each other task the body numbers, for the Inspector only.
    sources: Vec<aura_context_domain::PromptInspectorActivitySourceView>,
}

impl FrozenActivity {
    /// The activity fragment, or `None` when there is nothing truthful to add.
    ///
    /// `None` covers three different real situations, and it is the *coverage* that tells
    /// them apart rather than the absence itself: a healthy index with no activity, an index
    /// that could not answer, and a projection that relevance filtered to nothing.
    #[cfg(test)]
    pub(super) fn fragment(&self) -> anyhow::Result<Option<ContextFragment>> {
        if !self.summary.is_sendable() {
            return Ok(None);
        }
        Ok(self.fragments()?.into_iter().next())
    }

    /// Metadata and private fields retain separate manifest entries and source classes.
    pub(super) fn fragments(&self) -> anyhow::Result<Vec<ContextFragment>> {
        Ok(self.render()?.fragments)
    }

    /// Which session runs each other task the body numbers. The model never receives it.
    pub(super) fn sources(&self) -> Vec<aura_context_domain::PromptInspectorActivitySourceView> {
        self.render().map(|rendered| rendered.sources).unwrap_or_default()
    }

    fn render(&self) -> anyhow::Result<RenderedActivity> {
        if !self.summary.is_sendable() {
            return Ok(RenderedActivity {
                fragments: Vec::new(),
                shown: 0,
                extra_omitted: 0,
                sources: Vec::new(),
            });
        }
        let mut metadata = Vec::new();
        let mut workspace = Vec::new();
        let mut personal = Vec::new();
        let mut omitted = 0;
        let mut sources = Vec::new();
        // A row is an indivisible bundle. Its private fields never lose their original class.
        // The budget includes the real headers, JSON escaping and all fragments.
        for selected in &self.summary.rows {
            let row = &selected.row;
            let own = selected.relevance == aura_activity_runtime::ActivityRelevance::Direct;
            let targets = row
                .resources
                .iter()
                .map(|resource| {
                    use aura_activity_domain::ActivityResourceTarget::*;
                    let identity = match &resource.target {
                        VerifiedObject { identity, subtree, .. } => {
                            serde_json::json!({"identity":identity,"subtree":subtree})
                        }
                        VerifiedEntry { parent, .. } => {
                            serde_json::json!({"parent":parent,"prospective":true})
                        }
                        Unproven { .. } => serde_json::json!({"unproven":true}),
                        DirectoryEntry { parent_volume_serial, parent_file_index, .. } => serde_json::json!({"parent_volume_serial":parent_volume_serial,"parent_file_index":parent_file_index,"prospective":true}),
                    target => serde_json::to_value(target).unwrap_or_default(),
                    };
                    serde_json::json!({"target":identity,"relation":resource.relation})
                })
                .collect::<Vec<_>>();
            let names = row
                .resources
                .iter()
                .map(|resource| serde_json::json!({"display_name":resource.display_name,"truncated":resource.display_name_truncated}))
                .collect::<Vec<_>>();
            let named = row.resources.iter().any(|resource| !resource.display_name.is_empty());
            // Another session's task is named by what it may touch, never by its session, tool or
            // id: this step can neither query nor stop it, only keep clear of what it writes. Its
            // number is local to this body and pairs the row with its classified names.
            let (public, private) = if own {
                let tool_call_id = match &row.execution {
                    aura_activity_domain::ActivityExecutionRef::AgentToolCall {
                        tool_call_id,
                        ..
                    } => Some(*tool_call_id),
                    _ => None,
                };
                (
                    serde_json::json!({"activity_id":row.id,"tool_call_id":tool_call_id,"tool":if row.tool_name.is_empty(){row.tool_id.to_string()}else{row.tool_name.clone()},
                        "group":row.tool_group,"started_at":row.started_at,"targets":targets}),
                    (!row.parameters.is_empty() || named).then(|| {
                        serde_json::json!({"activity_id":row.id,"resources":names,"parameters":row.parameters})
                    }),
                )
            } else {
                let another_task = u32::try_from(sources.len() + 1).unwrap_or(u32::MAX);
                sources.push(aura_context_domain::PromptInspectorActivitySourceView {
                    another_task,
                    session_id: row.session_id,
                    workspace_id: row.workspace_id,
                });
                (
                    serde_json::json!({"another_task":another_task,"targets":targets}),
                    named.then(
                        || serde_json::json!({"another_task":another_task,"resources":names}),
                    ),
                )
            };
            metadata.push(public);
            if let Some(value) = private.as_ref() {
                match row.private_source {
                    aura_activity_domain::ActivityPrivateSource::WorkspaceMetadata => {
                        workspace.push(value.clone())
                    }
                    aura_activity_domain::ActivityPrivateSource::PersonalData => {
                        personal.push(value.clone())
                    }
                }
            }
            let bytes = self.header_with_counts(metadata.len(), self.summary.rows.len()).len()
                + serde_json::to_vec(&metadata)?.len()
                + serde_json::to_vec(&workspace)?.len()
                + serde_json::to_vec(&personal)?.len()
                + 4;
            if bytes > aura_activity_domain::MAX_ACTIVITY_SUMMARY_BYTES {
                metadata.pop();
                if !own {
                    sources.pop();
                }
                if private.is_some() {
                    match row.private_source {
                        aura_activity_domain::ActivityPrivateSource::WorkspaceMetadata => {
                            workspace.pop();
                        }
                        aura_activity_domain::ActivityPrivateSource::PersonalData => {
                            personal.pop();
                        }
                    }
                }
                omitted += 1;
            }
        }
        let mut content = self.header_with_counts(metadata.len(), omitted);
        content.push('\n');
        content.push_str(&serde_json::to_string(&metadata)?);
        let mut fragments = vec![self.make_fragment(
            "agent.activity_index.v1",
            NetworkDataClass::PublicContent,
            content,
        )];
        if !workspace.is_empty() {
            fragments.push(self.make_fragment(
                "agent.activity_index.workspace.v1",
                NetworkDataClass::WorkspaceMetadata,
                serde_json::to_string(&workspace)?,
            ));
        }
        if !personal.is_empty() {
            fragments.push(self.make_fragment(
                "agent.activity_index.personal.v1",
                NetworkDataClass::PersonalData,
                serde_json::to_string(&personal)?,
            ));
        }
        Ok(RenderedActivity {
            fragments,
            shown: u32::try_from(metadata.len()).unwrap_or(u32::MAX),
            extra_omitted: u64::try_from(omitted).unwrap_or(u64::MAX),
            sources,
        })
    }

    fn make_fragment(
        &self,
        kind: &str,
        class: NetworkDataClass,
        content: String,
    ) -> ContextFragment {
        ContextFragment {
            source_ref: ContextSourceRef::RuntimeState {
                state_kind: kind.into(),
                state_revision: self.revision,
            },
            source_kind: ModelEgressSourceKind::ActivityIndex,
            data_class: class,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content,
        }
    }

    /// The exact bytes that become the fragment body.
    ///
    /// The header line carries the coverage state, so a reader can tell a busy runtime from a
    /// blind one without a second request. The sampling instant, the runtime and the revision
    /// stay in the inspector's view and out of the text: they change every step, and the task
    /// state that holds this fragment has to stay the same from one step to the next for a
    /// provider's prompt cache. It is a fixed, bounded preamble followed by the summarised rows;
    /// malicious row text stays data inside the body and cannot introduce a new instruction
    /// block, because the section serializer escapes it and the fragment is one `Source` element.
    pub(super) fn rendered_body(&self) -> String {
        self.fragments()
            .unwrap_or_default()
            .iter()
            .map(|fragment| fragment.content.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn header_with_counts(&self, shown: usize, extra_omitted: usize) -> String {
        let header = &self.summary.header;
        let coverage = match header.coverage.state {
            ActivityCoverageState::Complete => "complete",
            ActivityCoverageState::Partial => "partial",
            ActivityCoverageState::Unavailable => "unavailable",
        };
        let body = format!(
            "coverage={} in_scope={} shown={} omitted={}",
            coverage,
            header.coverage.total_in_scope,
            shown,
            coverage_omitted(&header.coverage)
                .saturating_add(u64::try_from(extra_omitted).unwrap_or(u64::MAX)),
        );
        format!("{body}; read-only observation; not authorization")
    }

    /// Whether the model step's fragment must not be built at all.
    pub(super) fn is_unavailable(&self) -> bool {
        self.summary.header.coverage.state == ActivityCoverageState::Unavailable
    }

    /// Whether this frozen value belongs to the given model step.
    ///
    /// The transport retry loop re-sends one already-compiled request, so "is this the same
    /// step's picture" is answerable from the frozen values themselves. A caller that finds
    /// `false` has crossed a step boundary and must sample again; a caller that finds `true`
    /// must reuse these bytes rather than re-reading the index.
    #[must_use]
    pub(super) fn belongs_to_step(&self, model_step_id: ModelStepId) -> bool {
        self.model_step_id == model_step_id
    }
}

fn coverage_omitted(coverage: &aura_activity_domain::ActivityCoverage) -> u64 {
    [
        coverage.omitted_by_limit,
        coverage.capacity_dropped,
        coverage.unavailable_dropped,
        coverage.rejected,
        coverage.visibility_filtered,
    ]
    .into_iter()
    .fold(0_u64, u64::saturating_add)
}

impl AgentApplication {
    /// Freezes the activity snapshot for one model step.
    ///
    /// It is called exactly once per `prepare`, which is exactly once per model step. A
    /// transport retry never reaches here: the retry loop re-sends the request the first
    /// attempt already compiled.
    ///
    /// R4: THE VISIBILITY DECISION COMES FIRST, AND IT IS NOT THE QUERY'S TO MAKE.
    /// The reader is the session this step really runs as, in the workspace it really belongs to,
    /// taken from the run's own facts - never from the fact that a question looks broad. The
    /// previous shape set `cross_session_visible = true` unconditionally, and `may_see` then
    /// admitted **every** other session's private body into the model's context. What one reader
    /// may see is the index's `may_see`, applied before relevance and before any budget, so a
    /// same-workspace sibling session keeps its private paths to itself while another session's
    /// unknown-write-set row stays in scope and is reported as filtered rather than vanishing.
    pub(super) fn freeze_activity_summary(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        run_id: AgentRunId,
        model_step_id: ModelStepId,
    ) -> FrozenActivity {
        let mut visibility =
            ActivityVisibility::for_session(self.execution.runtime, session_id, workspace_id);
        visibility.relevant_targets = self.run_relevant_targets(run_id);
        let references = self.execution.task_resource_references(run_id);
        visibility.relevant_names = references
            .iter()
            .filter(|resource| !resource.display_name.is_empty())
            .map(|resource| {
                (
                    aura_activity_runtime::ActivityResourceKey::of(&resource.target),
                    resource.display_name.clone(),
                )
            })
            .collect();
        visibility.relevant_resources = references
            .into_iter()
            .take(aura_activity_domain::MAX_ACTIVITY_RELEVANT_HINTS)
            .map(|resource| resource.target)
            .collect();
        let snapshot = self.execution.activity_index.snapshot(&visibility);
        let summary = snapshot.select_summary(&visibility, ActivitySummaryBudget::default());
        FrozenActivity { summary, model_step_id, revision: snapshot.header.revision }
    }

    /// The physical targets the current task can really see (R3).
    ///
    /// WHERE THE INPUT COMES FROM, STATED EXPLICITLY
    /// It is the union of the resource references the run's own operations **already carry**: the
    /// verified descriptors on this step's secured operations, which A resolved before the call was
    /// admitted. Nothing here reads the journal, re-opens a path or waits for a lock, and nothing
    /// consults a *running* row - a run's row is removed when the call ends, so it cannot serve as
    /// the task's resource history.
    ///
    /// That removal is exactly why the previous implementation returned an empty set in practice:
    /// it re-sampled the index under freshly generated `SessionId::new()`/`WorkspaceId::new()`, so
    /// no real row could ever be in scope, and a run whose call had already finished had no row
    /// left to read. The set is built only from descriptors this run really holds, so it cannot be
    /// widened by a caller and it is the same set the task itself would use to justify touching the
    /// file.
    fn run_relevant_targets(
        &self,
        run_id: AgentRunId,
    ) -> std::collections::BTreeSet<aura_activity_runtime::ActivityResourceKey> {
        self.execution
            .task_resource_references(run_id)
            .iter()
            // One conversion, shared with the publishers: a descriptor that names no physical
            // object (a network origin) contributes no key rather than a fabricated one.
            .map(|resource| aura_activity_runtime::ActivityResourceKey::of(&resource.target))
            .collect()
    }

    /// The Inspector's real projection of the frozen activity.
    ///
    /// It reports the same facts the fragment was built from - the sampling instant, the
    /// runtime, the revision, the coverage and the truncation - so the Inspector never
    /// disagrees with the body that was actually sent.
    pub(super) fn activity_inspector_view(
        frozen: &FrozenActivity,
        inspector: &mut PromptInspectorInput,
    ) {
        let rendered = frozen.render().expect("typed bounded activity rows serialize");
        inspector.activity = Some(aura_context_domain::PromptInspectorActivityView {
            sampled_at: frozen.summary.header.sampled_at,
            runtime_instance_id: frozen.summary.header.runtime_instance_id,
            revision: frozen.summary.header.revision,
            coverage: frozen.summary.header.coverage.state,
            rows_shown: rendered.shown,
            rows_in_scope: frozen.summary.header.coverage.total_in_scope,
            omitted: coverage_omitted(&frozen.summary.header.coverage)
                .saturating_add(rendered.extra_omitted),
            truncated: frozen.summary.header.coverage.truncated || rendered.extra_omitted > 0,
            body_hash: aura_activity_domain::summary_content_hash(&frozen.rendered_body()),
            fragment_present: !frozen.is_unavailable() && frozen.summary.is_sendable(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one property that makes the retry contract checkable: the body is a pure function
    /// of the frozen summary, so rendering it twice produces the same bytes.
    #[test]
    fn the_rendered_body_is_stable_for_one_frozen_summary() {
        let frozen = FrozenActivity {
            summary: ActivitySummary {
                header: aura_activity_domain::ActivitySnapshotHeader {
                    sampled_at: UtcTimestamp::now(),
                    runtime_instance_id: RuntimeInstanceId::new(),
                    revision: aura_domain_common::Revision::new(7),
                    coverage: aura_activity_domain::ActivityCoverage::complete(0, 0),
                    stale: false,
                },
                rows: Vec::new(),
                body: String::new(),
            },
            model_step_id: ModelStepId::new(),
            revision: aura_domain_common::Revision::new(7),
        };
        let first = frozen.rendered_body();
        let second = frozen.rendered_body();
        assert_eq!(first, second, "a frozen body must not drift between renders");
        assert!(first.contains("coverage=complete"));
        assert!(!first.contains("revision="), "nothing in it changes from step to step");
        // A complete, empty picture is still *sendable*: it tells the model that the index
        // really answered and found nothing, which is not the same as an index that is blind.
        assert!(frozen.summary.is_sendable());
        // An unavailable one is not, and produces no fragment at all.
        let unavailable = FrozenActivity {
            summary: ActivitySummary {
                header: aura_activity_domain::ActivitySnapshotHeader {
                    coverage: aura_activity_domain::ActivityCoverage::unavailable(),
                    ..frozen.summary.header.clone()
                },
                rows: Vec::new(),
                body: String::new(),
            },
            model_step_id: frozen.model_step_id,
            revision: frozen.revision,
        };
        assert!(!unavailable.summary.is_sendable());
        assert!(unavailable.fragment().expect("no error").is_none());
    }
}
