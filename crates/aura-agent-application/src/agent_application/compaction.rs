//! Replace covered history with one verified background from a single model request.
mod attempts;
mod background;
mod boundary;
mod bounded_prefix;
pub(super) mod dependencies;
mod prefix;
mod selection;
mod summary;

use super::*;
use aura_context_domain::*;
use aura_context_runtime::*;
use aura_persistence_v2::context::compaction as store;
pub(super) use background::{AuthorizedCheckpoint, BackgroundUse, apply, authorized_checkpoint};
pub(super) use selection::event_id;
use std::{future::Future, pin::Pin};

/// The compaction runtime's view of the application for one compaction operation.
///
/// It holds the application and the run's cancellation token **by value** rather than by
/// reference, because the four runtime traits it implements are awaitable and their futures must
/// be able to move their real synchronous store work onto the bounded Agent Store owner. The token
/// is a cheap handle over an `Arc` and `is_cancelled` observes the same flag it always did.
struct Execution {
    app: Arc<AgentApplication>,
    cancel: ModelCancellationToken,
    record: store::Compaction,
    identity: aura_security_contract::InvocationIdentity,
    template: ModelExecutionSnapshot,
    endpoint: ModelEndpointConfig,
    prefix: prefix::PrefixAttempt,
    source_access: dependencies::Access,
}
pub(super) struct CompactionRequest<'a> {
    pub budget: &'a ContextBudgetSnapshot,
    pub non_history_tokens: u64,
    pub manual: Option<&'a ManualCompactionRequestRecord>,
    pub prefix: &'a RequestPrefix,
    pub source_access: dependencies::Access,
    pub missing_adopted: &'a std::collections::BTreeSet<EventId>,
    pub fixed_tool_ids: &'a [ToolId],
    pub resolved_attachments: Arc<[aura_model_domain::ResolvedModelAttachment]>,
}
/// What a compaction pass did. Finding nothing to compact is an answer, not a failure:
/// the caller decides whether the request is sent as it is or the user is asked.
pub(super) enum Compacted {
    Done(Option<CompactionCacheUsage>),
    NothingEligible,
    FellShort { code: String },
}
impl AgentApplication {
    pub(super) async fn compact(
        self: &Arc<Self>,
        input: &ContextPreparationInput,
        snapshot: &ModelExecutionSnapshot,
        endpoint: &ModelEndpointConfig,
        request: CompactionRequest<'_>,
        cancel: &ModelCancellationToken,
    ) -> anyhow::Result<Compacted> {
        let step_id = input.request.model_step_id;
        let session_id = input.request.session_id;
        if let Some(existing) = self
            .store
            .read(
                StoreStep::run(input.request.agent_run_id, "agent.compaction.prior_operation"),
                move |db| store::existing_for_step(db, session_id, step_id),
            )
            .await?
        {
            if existing.state == "completed" {
                return Ok(Compacted::Done(None));
            }
            if existing.state == "running"
                && !cancel.is_cancelled()
                && dependencies::visible(&existing.background_sources, &request.source_access)
            {
                let id = existing.id;
                let journal = Arc::clone(&self.execution.journal);
                let restored = self
                    .store
                    .run_critical(
                        StoreStep::run(
                            input.request.agent_run_id,
                            "agent.compaction.restore_receipt",
                        ),
                        StoreAccess::Write,
                        move || {
                            Ok(journal.lock()?.database.unit_of_work(|tx| {
                                let current = store::load(tx, id)?;
                                store::recover_received(tx, &current, UtcTimestamp::now())
                            })?)
                        },
                    )
                    .await?;
                if restored {
                    return Ok(Compacted::Done(None));
                }
            }
            return Ok(Compacted::FellShort {
                code: format!("context.compaction_existing_operation.{}", existing.state),
            });
        }
        let core = self.compaction_instructions(input).await?;
        let goal = snapshot.historical_snapshot_max_tokens;
        let core = format!(
            "{core}\nAim for about {goal} tokens when needed. A short task needs fewer; preserve important facts even if the background is longer."
        );
        let raw = request.prefix;
        // A manual compaction can extend the last adopted request with its question, retaining
        // the exact previously sent state and tools. Fresh sources stay beyond its watermark.
        let needs_complete_prefix = !request.missing_adopted.is_empty();
        let needs_bounded_prefix = input.tool_continuation_facts.is_some()
            || raw.estimated_input_tokens > request.budget.request_input_budget_tokens;
        let mut boundary =
            if request.manual.is_some() || needs_complete_prefix || needs_bounded_prefix {
                boundary::previous(
                    self,
                    input,
                    raw,
                    snapshot,
                    request.fixed_tool_ids,
                    needs_complete_prefix,
                )
                .await?
            } else {
                None
            };
        if needs_complete_prefix && boundary.is_none() {
            return Ok(Compacted::FellShort {
                code: "context.adopted_history_prefix_unavailable".to_owned(),
            });
        }
        let input_ref = Arc::new(input.clone());
        let select = |boundary: Option<&boundary::Admitted>| {
            selection::select(
                Arc::clone(self),
                Arc::clone(&input_ref),
                request.non_history_tokens,
                request.budget.clone(),
                request.manual.is_none(),
                boundary.map(|b| selection::AdoptedHistory {
                    cutoff: b.cutoff,
                    events: b.events.clone(),
                    items_for_read_gap: needs_complete_prefix
                        .then(|| b.prefix.conversation_items.clone()),
                }),
                request.missing_adopted.clone(),
            )
        };
        let mut sources = select(boundary.as_ref()).await?;
        if sources.is_none() && !needs_complete_prefix && boundary.is_some() {
            boundary = None;
            sources = select(None).await?;
        }
        let Some(sources) = sources else {
            if let Some(manual) = request.manual {
                // Boundary: consuming the manual request with no eligible history.
                let journal = Arc::clone(&self.execution.journal);
                let manual = manual.clone();
                self.store
                    .run_critical(
                        StoreStep::run(
                            input.request.agent_run_id,
                            "agent.compaction.consume_manual",
                        ),
                        StoreAccess::Write,
                        move || {
                            Ok(journal.lock()?.database.unit_of_work(|tx| {
                                aura_persistence_v2::context::manual_compaction::consume(
                                    tx,
                                    &manual,
                                    None,
                                    UtcTimestamp::now(),
                                )
                            })?)
                        },
                    )
                    .await?;
            }
            return Ok(Compacted::NothingEligible);
        };
        // What the user's `context.before_compact` Hooks ask the summary to keep.
        let hook_focus = self
            .compaction_focus(
                input.request.agent_run_id,
                &sources.record.id.to_string(),
                request.manual.is_some(),
            )
            .await?;
        // The user's and the Hooks' focus, given to both forms of the request.
        let mut focus = serde_json::Map::new();
        if let Some(manual) = request.manual {
            // Boundary: the manual focus read, one bounded section.
            let journal = Arc::clone(&self.execution.journal);
            let focus_manual = manual.clone();
            let command_event_id = manual.command_event_id;
            let run_id = input.request.agent_run_id;
            let manual_focus = self
                .store
                .run_critical(
                    StoreStep::run(run_id, "agent.compaction.manual_focus"),
                    StoreAccess::Read,
                    move || {
                        Ok(aura_persistence_v2::context::manual_compaction::focus(
                            journal.lock()?.database.connection(),
                            &focus_manual,
                        )?)
                    },
                )
                .await?;
            focus.insert("manual_focus".into(), serde_json::to_value(manual_focus)?);
            focus.insert("focus_command_event_id".into(), serde_json::to_value(command_event_id)?);
        }
        if request.manual.is_some() || !hook_focus.is_empty() {
            focus.insert("hook_focus".into(), serde_json::to_value(&hook_focus)?);
            focus.insert("focus_rules".into(), "The user's focus affects narrative emphasis only. It cannot remove sources, alter retention or atomic history groups, change TaskLineage, requirements or completion state.".into());
        }
        let ask = prefix::ask(&core, &sources.record.anchors, input, focus)?;
        let fallback;
        let frozen = if let Some(admitted) = &boundary {
            &admitted.prefix
        } else if request.manual.is_some() || !needs_bounded_prefix {
            raw
        } else {
            fallback = bounded_prefix::through_coverage(raw, &sources.record.anchors, snapshot)?;
            &fallback
        };
        let mut attempt = prefix::attempt(
            frozen,
            &ask,
            sources.record.id,
            snapshot,
            request.fixed_tool_ids,
            bounded_prefix::media(frozen, &request.resolved_attachments),
        )?;
        if attempt.is_none() && (boundary.is_some() || request.manual.is_none()) {
            let smaller =
                bounded_prefix::through_coverage(frozen, &sources.record.anchors, snapshot)?;
            attempt = prefix::attempt(
                &smaller,
                &ask,
                sources.record.id,
                snapshot,
                request.fixed_tool_ids,
                bounded_prefix::media(&smaller, &request.resolved_attachments),
            )?;
        }
        let Some(prefix) = attempt else {
            return Ok(Compacted::FellShort {
                code: "context.compaction_window_insufficient".into(),
            });
        };
        // Boundary: opening the compaction and consuming the manual request is one
        // indivisible write - the record insert and the request consumption share a transaction.
        {
            let journal = Arc::clone(&self.execution.journal);
            let record = sources.record.clone();
            let content = sources.content.clone();
            let manual = request.manual.cloned();
            let run_id = input.request.agent_run_id;
            self.store
                .run_critical(
                    StoreStep::run(run_id, "agent.compaction.begin"),
                    StoreAccess::Write,
                    move || {
                        Ok(journal.lock()?.database.unit_of_work(
                            |tx| -> aura_persistence_core::StoreResult<()> {
                                store::begin(tx, &record, content.as_bytes())?;
                                if let Some(manual) = &manual {
                                    aura_persistence_v2::context::manual_compaction::consume(
                                        tx,
                                        manual,
                                        Some(&record),
                                        UtcTimestamp::now(),
                                    )?;
                                }
                                Ok(())
                            },
                        )?)
                    },
                )
                .await?;
        }
        let operation = ContextCompactionOperationSpec {
            session_id: input.request.session_id,
            agent_run_id: input.request.agent_run_id,
            model_profile_id: snapshot.model_profile_id,
            source_context_snapshot_id: sources.record.id,
            sequence: sources.record.sequence,
            source_span: sources.record.source_span.clone(),
            anchor_manifest: sources.record.anchors.clone(),
            output_character_limit: goal,
            retained_recent_tokens: sources.record.retained_recent_tokens,
            retention_exceptions: sources.record.retention_exceptions.clone(),
        };
        let mut template = snapshot.clone();
        if let Some(admitted) = &boundary {
            template.historical_tool_call_wire_bindings.clone_from(&admitted.wire_bindings);
        }
        let execution = Execution {
            app: Arc::clone(self),
            cancel: cancel.clone(),
            record: sources.record,
            identity: input.request.invocation_identity.clone(),
            template,
            endpoint: endpoint.clone(),
            prefix,
            source_access: request.source_access,
        };
        let compaction = execution.record.id.to_string();
        let result =
            execute_context_compaction(operation, &execution, &execution, &execution, &execution)
                .await;
        match result {
            ContextCompactionExecutionOutcome::Completed { .. } => {
                self.after_compaction(input.request.agent_run_id, &compaction).await?;
                let id = execution.record.id;
                let reusable_prefix_tokens = frozen.estimated_input_tokens;
                let stats = self
                    .store
                    .read(
                        StoreStep::run(input.request.agent_run_id, "agent.compaction.metrics"),
                        move |db| {
                            let saved = store::load(db, id)?;
                            let snapshot = saved
                                .attempts
                                .last()
                                .ok_or(aura_persistence_core::PersistenceCoreError::Conflict)?
                                .model_execution_snapshot_id;
                            let usage = store::output(db, snapshot)?.usage;
                            let reported =
                                usage.source == aura_model_domain::UsageSource::ProviderReported;
                            Ok(CompactionCacheUsage {
                                reusable_prefix_tokens,
                                reported_input_tokens: reported.then_some(usage.input_tokens),
                                reported_cached_tokens: usage
                                    .cached_input_tokens
                                    .filter(|_| reported),
                            })
                        },
                    )
                    .await?;
                aura_core_base::append_core_log(
                    "context.compaction.summary_usage",
                    serde_json::json!({
                        "compaction": id, "source_step": input.request.model_step_id, "usage": stats
                    }),
                );
                Ok(Compacted::Done(Some(stats)))
            }
            ContextCompactionExecutionOutcome::Failed { failure, .. } => {
                Ok(Compacted::FellShort { code: failure.code })
            }
            other => anyhow::bail!("context.compaction_failed: {other:?}"),
        }
    }

    /// The reviewed compaction instructions of the run's frozen prompt bundle. The user's
    /// addendum is not promoted to the auxiliary execution contract.
    async fn compaction_instructions(
        self: &Arc<Self>,
        input: &ContextPreparationInput,
    ) -> anyhow::Result<String> {
        // Boundary: reading the frozen prompt bundle and its artifacts is one synchronous
        // section; `frozen_prompt` takes a synchronous reader, so the section owns the guard.
        let (bundle, _settings) = {
            let journal = Arc::clone(&self.execution.journal);
            let bundle_snapshot = input.prompt_bundle_snapshot.clone();
            self.store
                .run_critical(
                    StoreStep::run(input.request.agent_run_id, "agent.compaction.prompt_bundle"),
                    StoreAccess::Read,
                    move || {
                        let journal = journal.lock()?;
                        let db = journal.database.connection();
                        super::context::frozen_prompt(&bundle_snapshot, |id| {
                            Ok(aura_persistence_core::artifact::read(
                                db,
                                id,
                                &bundle_snapshot.id.to_string(),
                            )?)
                        })
                    },
                )
                .await?
        };
        summary::instructions(&bundle)
    }
}
impl CompactionModelCaller for Execution {
    fn call<'a>(
        &'a self,
        attempt: &'a PreparedCompactionModelAttempt,
    ) -> Pin<Box<dyn Future<Output = CompactionModelCallTerminal> + Send + 'a>> {
        Box::pin(async move {
            let request = super::provider::Request {
                p05: self.app.p05.clone(),
                endpoint: self.endpoint.clone(),
                store: Arc::clone(&self.app.store),
                identity: self.identity.clone(),
                logical: attempt.logical_request.clone(),
                snapshot: attempt.execution_snapshot.clone(),
                manifest: attempt.egress_manifest.clone(),
                attachments: Arc::clone(&self.prefix.resolved_attachments),
                single_attempt: true,
                backup_waiting: false,
                idle_retry: false,
            };
            match super::provider::execute(&self.app, request, &self.cancel).await {
                Ok(TransportRunTerminal::Completed { output, attempts }) => match attempts.last() {
                    Some(a) => CompactionModelCallTerminal::Completed {
                        output: Box::new(output),
                        transport_attempt_id: a.id,
                    },
                    None => CompactionModelCallTerminal::Failed {
                        error_code: "context.compaction_transport_missing".into(),
                        transport_attempt_id: None,
                        retryable: false,
                    },
                },
                Ok(TransportRunTerminal::Cancelled { .. }) => {
                    CompactionModelCallTerminal::Cancelled {
                        error_code: "context.compaction_cancelled".into(),
                        transport_attempt_id: None,
                    }
                }
                Ok(TransportRunTerminal::Failed { failure, .. }) => {
                    let failure = failure.failure;
                    CompactionModelCallTerminal::Failed {
                        retryable: matches!(
                            failure.category,
                            ModelFailureCategory::Transport
                                | ModelFailureCategory::StreamInterrupted
                                | ModelFailureCategory::Protocol
                                | ModelFailureCategory::EmptyOutput
                        ),
                        error_code: failure.code,
                        transport_attempt_id: failure.transport_attempt_id,
                    }
                }
                Err(error) => CompactionModelCallTerminal::Failed {
                    error_code: error.to_string(),
                    transport_attempt_id: None,
                    retryable: false,
                },
            }
        })
    }
}
impl CompactionLifecycleSink for Execution {
    fn mark_attempt_running<'a>(
        &'a self,
        operation: &'a ContextCompactionOperationSpec,
        a: &'a PreparedCompactionModelAttempt,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            // Boundary: the attempt record is one bounded write.
            let journal = Arc::clone(&self.app.execution.journal);
            let record_id = self.record.id;
            let snapshot = a.execution_snapshot.clone();
            let attempt_ordinal = a.attempt_ordinal;
            let sequence = operation.sequence;
            let run_id = self.record.agent_run_id;
            self.app
                .store
                .run_critical(
                    StoreStep::run(run_id, "agent.compaction.attempt_running"),
                    StoreAccess::Write,
                    move || {
                        journal
                            .lock()
                            .and_then(|mut journal| {
                                journal.database.unit_of_work(|tx| {
                                    store::start_attempt(
                                        tx,
                                        record_id,
                                        &snapshot,
                                        CompactionModelAttemptRecord {
                                            source_context_snapshot_id: record_id,
                                            compaction_sequence: sequence,
                                            attempt_ordinal,
                                            attempt_context_snapshot_id: snapshot
                                                .context_snapshot_id,
                                            model_step_id: snapshot.model_step_id,
                                            model_execution_snapshot_id: snapshot.id,
                                            transport_attempt_id: None,
                                            state: CompactionModelAttemptState::Running,
                                            error_code: None,
                                            started_at: UtcTimestamp::now(),
                                            terminal_at: None,
                                        },
                                    )
                                })
                            })
                            .map_err(|e| anyhow::anyhow!(e.to_string()))
                    },
                )
                .await
                .map_err(|e| e.to_string())
        })
    }
    fn finish_operation<'a>(
        &'a self,
        f: &'a CompactionAttemptFailureCommit,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { self.fail(f.clone()).await })
    }
    fn commit_completed<'a>(
        &'a self,
        c: &'a ValidatedCompactionCommit,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            if self.cancel.is_cancelled()
                || !dependencies::visible(&self.record.background_sources, &self.source_access)
            {
                let cancelled = self.cancel.is_cancelled();
                let code = if cancelled {
                    "context.compaction_cancelled"
                } else {
                    "context.compaction_sources_changed"
                };
                self.fail(CompactionAttemptFailureCommit {
                    operation: c.operation.clone(),
                    attempt: Some(c.attempt.clone()),
                    state: if cancelled {
                        CompactionModelAttemptState::Cancelled
                    } else {
                        CompactionModelAttemptState::Failed
                    },
                    transport_attempt_id: Some(c.transport_attempt_id),
                    error_code: code.into(),
                })
                .await?;
                return Err(code.into());
            }
            let result = ContextReadCompactionResult {
                source_context_snapshot_id: self.record.id,
                sequence: self.record.sequence,
                historical_snapshot: c.parsed.historical_snapshot.clone(),
                active_turn_summary: c.parsed.active_turn_summary.clone(),
                structured_compaction: c.parsed.structured_compaction.clone(),
                source_span: self.record.source_span.clone(),
                anchor_manifest: self.record.anchors.clone(),
            };
            // Boundary: the completed-compaction commit is one bounded write.
            let journal = Arc::clone(&self.app.execution.journal);
            let record_id = self.record.id;
            let attempt_ordinal = c.attempt.attempt_ordinal;
            let transport_attempt_id = c.transport_attempt_id;
            let run_id = self.record.agent_run_id;
            self.app
                .store
                .run_critical(
                    StoreStep::run(run_id, "agent.compaction.commit_completed"),
                    StoreAccess::Write,
                    move || {
                        journal
                            .lock()
                            .and_then(|mut j| {
                                j.database.unit_of_work(|tx| {
                                    store::complete(
                                        tx,
                                        record_id,
                                        attempt_ordinal,
                                        transport_attempt_id,
                                        result,
                                        UtcTimestamp::now(),
                                    )
                                })
                            })
                            .map_err(|e| anyhow::anyhow!(e.to_string()))
                    },
                )
                .await
                .map_err(|e| e.to_string())
        })
    }
}
impl Execution {
    async fn fail(&self, f: CompactionAttemptFailureCommit) -> Result<(), String> {
        let journal = Arc::clone(&self.app.execution.journal);
        let record_id = self.record.id;
        let agent_run_id = self.record.agent_run_id;
        // Boundary: the failure commit for this attempt is one bounded write.
        self.app
            .store
            .run_critical(
                StoreStep::run(agent_run_id, "agent.compaction.fail"),
                StoreAccess::Write,
                move || {
                    journal
                        .lock()
                        .and_then(|mut j| match &f.attempt {
                            Some(a) => j.database.unit_of_work(|tx| {
                                store::finish_attempt(
                                    tx,
                                    record_id,
                                    a.attempt_ordinal,
                                    f.state,
                                    f.transport_attempt_id,
                                    Some(f.error_code.clone()),
                                    UtcTimestamp::now(),
                                )
                            }),
                            None => j.database.unit_of_work(|tx| {
                                store::interrupt_run(tx, agent_run_id, false, UtcTimestamp::now())
                            }),
                        })
                        .map_err(|e| anyhow::anyhow!(e.to_string()))
                },
            )
            .await
            .map_err(|e| e.to_string())
    }
}
impl ModelEgressAuthorizationFacts for Execution {
    fn session_model_channel_is_authorized(
        &self,
        session: SessionId,
        profile: ModelProfileId,
    ) -> bool {
        session == self.record.session_id && profile == self.template.model_profile_id
    }
    async fn persisted_event(
        &self,
        id: EventId,
    ) -> Result<Option<PersistedContextEventAuthorizationFact>, ModelEgressAuthorizationError> {
        // A compaction on the step's prefix sends the conversation's user messages; each is
        // checked against the text stored for it, as in the step's own request (provider.rs).
        let session = self.record.session_id;
        let user = self
            .app
            .store
            .read(
                StoreStep::run(self.record.agent_run_id, "agent.compaction.user_event"),
                move |db| {
                    aura_persistence_v2::agent::model_context::egress_user_text(db, session, id)
                },
            )
            .await
            .map_err(|_| ModelEgressAuthorizationError::FactStore)?;
        Ok(user.map(|text| PersistedContextEventAuthorizationFact {
            event_id: id,
            session_id: session,
            actor: PersistedContextEventActor::User,
            content_hash: ContentHash::sha256(text.as_bytes()),
            byte_length: text.len() as u64,
            excerpt: aura_context_domain::user_event_excerpt(id, &text)
                .map(|view| (ContentHash::sha256(view.as_bytes()), view.len() as u64)),
            model_visible: true,
            explicitly_user_authored: true,
        }))
    }
    fn source_grant_binding_is_valid(
        &self,
        _: SessionId,
        _: &ContextSourceRef,
        _: ModelEgressSourceKind,
        _: aura_security_contract::NetworkDataClass,
        _: &SourceAuthorizationBinding,
    ) -> Result<bool, ModelEgressAuthorizationError> {
        Ok(false)
    }
    /// A picture the user sent with a message of this conversation, which the step's request
    /// carries and a compaction on it sends again (provider.rs).
    async fn user_attachment(
        &self,
        session: SessionId,
        artifact: ArtifactId,
    ) -> Result<Option<(ContentHash, u64)>, ModelEgressAuthorizationError> {
        if session != self.record.session_id {
            return Ok(None);
        }
        self.app
            .store
            .read(
                StoreStep::run(self.record.agent_run_id, "agent.compaction.user_attachment"),
                move |db| {
                    aura_persistence_v2::agent::attachments::sent_by_user(db, session, artifact)
                },
            )
            .await
            .map_err(|_| ModelEgressAuthorizationError::FactStore)
    }
}
