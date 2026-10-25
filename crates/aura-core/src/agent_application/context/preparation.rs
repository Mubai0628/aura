use super::*;
// The thinking level the conversation chose for its turn.
#[path = "reasoning.rs"]
mod reasoning;
// The browser's screenshots a model that takes images is sent.
#[path = "screenshots.rs"]
pub(super) mod screenshots;

impl AgentApplication {
    pub(in crate::agent_application) async fn prepare(
        self: &Arc<Self>,
        run_id: AgentRunId,
        profile: ModelProfileId,
        reasoning: Option<aura_agent_domain::TurnReasoningEffort>,
        continuation: AgentContinuation,
        cancel: &ModelCancellationToken,
    ) -> anyhow::Result<Option<Prepared>> {
        self.refresh_prompt(run_id).await?;
        let session = self.turn(run_id).await?.session_id;
        // A branch's older turns may still be on their way; the model is sent its history
        // only once it is whole.
        self.wait_for_branch_copy(run_id, session, cancel).await?;
        let permission = self.freeze_permission(session).await?;
        let at = UtcTimestamp::now();
        // Boundary: the step-opening transaction. Before this round it took the journal lock
        // on the reactive task polling this run; it now runs whole on the one bounded Agent Store
        // owner, the same journal and the same transaction.
        let step = self
            .store
            .run_critical(
                StoreStep::run(run_id, "agent.context.prepare_step"),
                StoreAccess::Write,
                {
                    let journal = Arc::clone(&self.execution.journal);
                    move || {
                        Ok(journal.lock()?.database.unit_of_work(|tx| {
                            aura_persistence_v2::agent::lifecycle::prepare_step(
                                tx,
                                run_id,
                                continuation,
                                at,
                            )
                        })?)
                    }
                },
            )
            .await?;
        let (
            turn,
            run,
            bundle,
            prompt,
            settings,
            history,
            registry,
            loads,
            tool_settings,
            package_tools,
            mut state,
        ) = self.assemble_frozen_facts(run_id, &permission, &step, at).await?;
        let config = self.p05.model_config.snapshot()?;
        let model_profile = config
            .registry
            .models
            .get(&profile)
            .ok_or_else(|| anyhow::anyhow!("model.profile_missing"))?
            .clone();
        // The provider's default connection, or the
        // next backup when the connections before it failed in this run's current streak.
        let order = aura_model_config::connection_order(&config.registry, &model_profile);
        let failed = self
            .store
            .read(StoreStep::run(run_id, "agent.context.failed_connections"), move |db| {
                aura_persistence_v2::model::transient_retry::failed_connections(db, run_id)
            })
            .await?;
        let routed = aura_model_config::next_connection(&order, &failed)
            .or_else(|| order.first().copied())
            .unwrap_or(model_profile.endpoint_id);
        let backup_waiting = order.iter().any(|id| *id != routed && !failed.contains(id));
        let endpoint = config
            .registry
            .endpoints
            .get(&routed)
            .ok_or_else(|| anyhow::anyhow!("model.profile_missing"))?
            .clone();
        if failed.iter().any(|id| order.contains(id)) && order.first() != Some(&routed) {
            let (step_id, name) = (step.model_step_id, endpoint.display_name.clone());
            self.store
                .run_critical(
                    StoreStep::run(run_id, "agent.context.backup_connection"),
                    StoreAccess::Write,
                    {
                        let journal = Arc::clone(&self.execution.journal);
                        move || {
                            Ok(journal.lock()?.database.unit_of_work(|tx| {
                                aura_persistence_v2::model::connections::record_backup(
                                    tx, step_id, &name, at,
                                )
                            })?)
                        }
                    },
                )
                .await?;
        }
        let credential =
            self.p05.configuration_credential_preflight(&endpoint)?.credential_binding.map(|v| {
                ModelExecutionCredentialBinding {
                    credential_id: v.credential_id,
                    credential_revision: v.credential_revision,
                    credential_binding_fingerprint: v.credential_binding_fingerprint,
                }
            });
        let bindings = super::wire_history::bind(
            history.wire_bindings.clone(),
            &ProviderToolCallReuseDomain {
                endpoint_id: endpoint.id,
                endpoint_security_fingerprint: aura_model_config::endpoint_fingerprint(&endpoint)?,
                api_format: endpoint.api_format,
            },
        );
        let mut snapshot = aura_model_config::ModelConfigService::freeze_execution_snapshot(
            &config,
            CreateModelExecutionSnapshot {
                model_step_id: step.model_step_id,
                model_profile_id: profile,
                context_snapshot_id: ContextProjectionSnapshotId::new(),
                prompt_bundle_snapshot_id: bundle.id,
                historical_tool_call_wire_bindings: bindings,
                credential_binding: credential,
                endpoint_id: (routed != model_profile.endpoint_id).then_some(routed),
            },
        )?;
        // The conversation's own thinking level for this turn, when the model offers it.
        snapshot.request_options.reasoning_effort = reasoning::turn_level(
            reasoning,
            &model_profile,
            endpoint.api_format,
            snapshot.request_options.reasoning_effort,
        );
        // Boundary: one bounded read of the newest context-window trial for this run.
        let window_trial = self
            .store
            .run_critical(
                StoreStep::run(run_id, "agent.context.window_trial"),
                StoreAccess::Read,
                {
                    let journal = Arc::clone(&self.execution.journal);
                    move || {
                        Ok(model::context_window::latest(
                            journal.lock()?.database.connection(),
                            run_id,
                        )?
                        .filter(|r| r.view.state == AgentContextWindowState::Preparing))
                    }
                },
            )
            .await?;
        let window_trial = match window_trial {
            Some(record)
                if self.p05.window_trial_matches(&record, &model_profile, &endpoint)? =>
            {
                Some(record)
            }
            _ => None,
        };
        if let Some(window) = window_trial.as_ref().and_then(|r| r.candidate_windows.last()) {
            // This is a newly allocated, still-unpublished operation snapshot.
            // A trial never edits the Profile or an earlier immutable snapshot.
            snapshot.runtime_effective_window_tokens = *window;
        }
        // The step's `context.before_build` and `model.before_call` Hooks, before anything
        // is frozen. A block ends the run with the Hook's name and reason on record.
        let hooks =
            self.step_hooks(run_id, &turn, step.model_step_id, &snapshot.request_options).await?;
        if hooks.blocked {
            self.interrupt(run_id, "hooks.step_blocked", false).await?;
            return Ok(None);
        }
        if hooks.temperature.is_some() {
            snapshot.request_options.temperature = hooks.temperature;
        }
        if hooks.top_p.is_some() {
            snapshot.request_options.top_p = hooks.top_p;
        }
        if let Some(tokens) = hooks.max_output_tokens {
            snapshot.request_options.effective_max_output_tokens = tokens;
        }
        let force_rebuild = window_trial.as_ref().is_some_and(|r| {
            matches!(
                r.view.correction.resolution,
                ContextWindowResolution::P06RebuildRequired { .. }
            )
        });
        let mut identity = InvocationIdentity::runtime_user(
            turn.workspace_id,
            turn.session_id,
            CorrelationId::new(),
        );
        identity.actor = InvocationActor::ParentAgent;
        identity.turn_task_id = Some(turn.turn_task_id);
        identity.agent_run_id = Some(run_id);
        let mut fragments = vec![];
        for item in &history.items {
            fragments.push(history_fragment(item)?);
        }
        // The activity fragment.
        //
        // It is a **separate fragment inside `CurrentTaskState`**, never inside
        // `agent.control_state.v1`, so the ordinary control-state refresh below - which rewrites
        // exactly one fragment by matching that state kind - cannot overwrite it. There is no
        // thirteenth section and no new user message: this is one more source in a section that
        // already supports several.
        //
        // The snapshot is frozen **once, here**, for this model step. `prepare` runs once per
        // step; the transport retry loop re-sends the request this call already compiled rather
        // than re-entering here, so a retry cannot re-sample and the bytes it sends do not drift.
        let activity_summary = self.freeze_activity_summary(
            turn.workspace_id,
            turn.session_id,
            run_id,
            step.model_step_id,
        );
        debug_assert!(
            activity_summary.belongs_to_step(step.model_step_id),
            "the frozen activity must belong to the step it was taken for"
        );
        let mut task_state_fragments = vec![ContextFragment {
            source_ref: ContextSourceRef::RuntimeState {
                state_kind: "agent.control_state.v1".into(),
                state_revision: turn.revision,
            },
            source_kind: ModelEgressSourceKind::AuraCore,
            data_class: NetworkDataClass::PublicContent,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content: serde_json::to_string(&prompt_state(&state))?,
        }];
        task_state_fragments.extend(activity_summary.fragments()?);
        // The Skills loaded in this turn, re-checked for this step.
        task_state_fragments.extend(self.loaded_skill_fragments(
            run_id,
            turn.workspace_id,
            turn.revision,
        )?);
        let mut sections = vec![
            section(
                ContextSectionKind::RecentHistory,
                fragments,
                ContextCompressibility::CompressibleHistory,
            ),
            section(
                ContextSectionKind::CurrentTaskState,
                task_state_fragments,
                ContextCompressibility::NeverCompress,
            ),
        ];
        if !hooks.injections.is_empty() {
            sections.push(section(
                ContextSectionKind::HookInjections,
                hooks
                    .injections
                    .iter()
                    .map(|text| ContextFragment {
                        source_ref: ContextSourceRef::RuntimeState {
                            state_kind: "hook.injection.v1".into(),
                            state_revision: turn.revision,
                        },
                        source_kind: ModelEgressSourceKind::Hook,
                        data_class: NetworkDataClass::UserFileContent,
                        authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                        content: text.clone(),
                    })
                    .collect(),
                ContextCompressibility::NeverCompress,
            ));
        }
        let (trigger, tool_facts, core_facts) =
            self.continuation_facts(run_id, &step, &turn, &history, &mut sections).await?;
        // An idle `/compact` task tells the model what Core did for it.
        let purpose = self
            .store
            .read(StoreStep::run(run_id, "agent.context.purpose"), move |db| {
                aura_persistence_v2::agent::application::purpose(db, run_id)
            })
            .await?;
        let activation = PromptActivationInput {
            agent_mode: match turn.mode {
                AgentTaskMode::Normal => PromptAgentMode::Normal,
                AgentTaskMode::Plan => PromptAgentMode::Plan,
                AgentTaskMode::Goal => PromptAgentMode::Goal,
            },
            loaded_capabilities: registry
                .schema_set
                .tools
                .iter()
                .flat_map(|s| {
                    let name = s.canonical_name.to_string();
                    if name.starts_with("memory.") {
                        vec![name, "memory".into()]
                    } else if name.starts_with("workspace_init.") {
                        vec![name, "workspace_init".into()]
                    } else if name.starts_with("commands.") {
                        vec![name, "command_authoring".into()]
                    } else if name.starts_with("web.") || name.starts_with("http.") {
                        // The web module, which named a capability nothing produced.
                        vec![name, "web".into()]
                    } else {
                        vec![name]
                    }
                })
                .chain(
                    (purpose == AgentTaskPurpose::Compaction)
                        .then(|| "compaction_command".to_owned()),
                )
                .collect(),
            child_agent: false,
            recovery_kinds: Default::default(),
            compaction: false,
            maintenance_kind: None,
        };
        let sources = assemble_prompt_context_sources(&prompt, &settings, &activation, sections)?;
        let estimators = ContextEstimatorRegistry::default();
        let (inspector_lineage, previous_snapshot, counted) = {
            // Boundary: one bounded read pair for the inspector view.
            let journal = Arc::clone(&self.execution.journal);
            let turn_task_id = turn.turn_task_id;
            let session_id = turn.session_id;
            self.store
                .run_critical(
                    StoreStep::run(run_id, "agent.context.inspector"),
                    StoreAccess::Read,
                    move || {
                        let journal = journal.lock()?;
                        let db = journal.database.connection();
                        Ok((
                            aura_persistence_v2::agent::inspector::active_lineage_view(
                                db,
                                turn_task_id,
                            )?,
                            aura_persistence_v2::agent::inspector::previous_context(
                                db, session_id,
                            )?,
                            aura_persistence_v2::agent::counted_request::latest(db, session_id)?,
                        ))
                    },
                )
                .await?
        };
        let mut input = ContextPreparationInput {
            request: ContextPreparationRequest {
                invocation_identity: identity.clone(),
                session_id: turn.session_id,
                turn_task_id: turn.turn_task_id,
                agent_run_id: run_id,
                model_step_id: step.model_step_id,
                model_profile_id: profile,
                runtime_effective_window_tokens: snapshot.runtime_effective_window_tokens,
                output_request: ModelOutputRequest {
                    format: ModelOutputFormat::Text,
                    effective_max_output_tokens: snapshot
                        .request_options
                        .effective_max_output_tokens,
                },
                history_cutoff: history.cutoff,
            },
            prompt_bundle_snapshot: bundle,
            active_prompt_module_ids: sources.active_prompt_module_ids,
            section_inputs: sources.section_inputs,
            recent_history_items: history.items,
            trigger,
            tool_continuation_facts: tool_facts,
            core_continuation_facts: core_facts,
            tool_schema_set: registry.schema_set.clone(),
            effective_tool_bindings: registry.effective_bindings.clone(),
            tools: registry
                .schema_set
                .tools
                .iter()
                .map(|s| ContextToolInput {
                    tool_id: s.tool_id,
                    compressibility: ContextCompressibility::NeverCompress,
                })
                .collect(),
            attachments: vec![],
            attachment_egress_sources: vec![],
            model_estimator_key: ModelEstimatorKey {
                provider_id: snapshot.provider_id,
                provider_model_name: snapshot.model_name.clone(),
            },
            provider_target: ContextProviderTargetV1 {
                provider_id: snapshot.provider_id,
                api_format: snapshot.api_format,
                endpoint_id: snapshot.endpoint_id,
                model_profile_id: profile,
                endpoint_fingerprint: snapshot.endpoint_fingerprint.clone(),
            },
            inspector: {
                // The Inspector records the **same** frozen snapshot the activity
                // fragment was built from, so the reported view and the bytes actually sent
                // cannot disagree. It is projected here, from `activity_summary`, and never
                // recomputed from a second read of the index.
                let mut inspector = PromptInspectorInput {
                    active_task_lineage: inspector_lineage,
                    attachments_resolved: true,
                    ..Default::default()
                };
                AgentApplication::activity_inspector_view(&activity_summary, &mut inspector);
                inspector
            },
            previous_measurement: counted.and_then(|counted| measured(counted, &snapshot)),
            previous_snapshot,
            created_at: at,
            compacted_source_proofs: Vec::new(),
            coverage_hops: Vec::new(),
        };
        super::compaction::apply(self, &mut input).await?;
        // The tool results the previous request showed cleared stay cleared.
        clearing::keep_cleared(&mut input)?;
        clearing::shorten_arguments(&mut input)?;
        let screenshots = self
            .screenshots(
                run_id,
                turn.session_id,
                &input.recent_history_items,
                &screenshots::current_messages(&input.trigger),
                snapshot.request_options.image_input,
            )
            .await?;
        // The tools the step always loads; a compaction on its prefix sends them too.
        let fixed_tool_ids = registry
            .catalog_entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry.load_state,
                    aura_tool_contract::ToolLoadStateV1::Core
                        | aura_tool_contract::ToolLoadStateV1::Pinned
                )
            })
            .map(|entry| entry.tool_id)
            .collect::<Vec<_>>();
        let Some((prepared, compiled_state)) = self
            .compile_context(compilation::CompilationInput {
                run_id,
                turn: &turn,
                step: &step,
                state,
                input,
                snapshot: &snapshot,
                endpoint: &endpoint,
                window_trial: &window_trial,
                force_rebuild,
                cancel,
                estimators,
                fixed_tool_ids: &fixed_tool_ids,
                screenshots: &screenshots,
            })
            .await?
        else {
            return Ok(None);
        };
        state = compiled_state;
        let context = prepared.context;
        snapshot.context_snapshot_id = context.context_snapshot_id;
        // The output limit the request goes out with is the room its window has left.
        snapshot.request_options.effective_max_output_tokens =
            context.output_request.effective_max_output_tokens;
        // The snapshot names the wire binding of exactly the historical calls this request still
        // carries. After compaction the summary replaces earlier calls, and keeping their bindings
        // failed the adapter's exact-binding check, so the first step after every compaction of a
        // span with tool calls was interrupted.
        let carried = context
            .conversation_items
            .iter()
            .filter_map(|item| match item {
                LogicalConversationItem::Assistant { tool_calls, .. } => Some(tool_calls),
                _ => None,
            })
            .flatten()
            .map(|call| call.tool_call_id)
            .collect::<std::collections::BTreeSet<_>>();
        snapshot
            .historical_tool_call_wire_bindings
            .retain(|binding| carried.contains(&binding.tool_call_id));
        let attachments = screenshots.resolved(&context.attachments)?;
        let logical = LogicalModelRequest {
            fixed_tool_ids,
            model_step_id: step.model_step_id,
            execution_snapshot_id: snapshot.id,
            context_snapshot_id: context.context_snapshot_id,
            instruction_envelope: context.instruction_envelope,
            conversation_items: context.conversation_items,
            tool_schema_set: context.tool_schema_set,
            attachments: context.attachments,
            provider_continuation: context.provider_continuation,
            output_request: context.output_request,
        };
        // Boundary: the final commit. It is one indivisible transaction - the goal-budget
        // check, the checkpoint CAS, the artifacts, the exposure head, the context and model
        // snapshots, the step and run transitions, the remediation clear and the window freeze -
        // and it is moved whole onto the one bounded owner, so the reactive task that called it is
        // not parked on the journal lock.
        //
        // Nothing inside the section is rewritten: same statements, same order, same single
        // `unit_of_work`, same checkpoint CAS. Everything the section needs is MOVED in and the
        // frozen context snapshot is handed back out, so no large domain value is cloned to satisfy
        // the `Send + 'static` requirement - a clone here would be a second copy of state that the
        // transaction is supposed to be deciding about.
        let frozen_context_snapshot = context.snapshot.clone();
        let frozen_model_snapshot = snapshot.clone();
        let artifacts = prepared.artifacts;
        let prompt_inspector = prepared.prompt_inspector;
        {
            let journal = Arc::clone(&self.execution.journal);
            self.store
                .run_critical(
                    StoreStep::run(run_id, "agent.context.commit"),
                    StoreAccess::Write,
                    move || {
                        Ok(journal.lock()?.database.unit_of_work(
                            |tx| -> aura_persistence_core::StoreResult<()> {
                                if turn.mode == AgentTaskMode::Goal
                                    && work::goal(tx, turn.turn_task_id)?.budget_exhausted()
                                {
                                    return Err(
                                        aura_persistence_core::PersistenceCoreError::Invalid(
                                            "agent.goal_budget_exhausted",
                                        ),
                                    );
                                }
                                let expected_revision: Revision =
                                    serde_json::from_value(state["state_revision"].clone())?;
                                let expected_hash: jcs::CanonicalHashV1 =
                                    serde_json::from_value(state["state_hash"].clone())?;
                                if checkpoint::current_state(tx, turn.turn_task_id)?
                                    != (expected_revision, expected_hash)
                                {
                                    return Err(
                                        aura_persistence_core::PersistenceCoreError::Conflict,
                                    );
                                }
                                for value in &artifacts {
                                    artifact::put(
                                        tx,
                                        value.artifact_id,
                                        &step.model_step_id.to_string(),
                                        &value.media_type,
                                        &value.bytes,
                                        at,
                                    )?;
                                }
                                crate::agent_execution::package_tools::store(tx, &package_tools)?;
                                let previous = tool::exposure::load(tx, &registry.scope)?;
                                if previous.as_ref().is_none_or(|v| v.head != registry.snapshot_ref)
                                {
                                    let next = tool::exposure::ExposureState {
                                        scope: registry.scope.clone(),
                                        revision: Revision::new(
                                            previous.as_ref().map_or(1, |v| v.revision.value() + 1),
                                        ),
                                        head: registry.snapshot_ref.clone(),
                                        loads,
                                    };
                                    tool::exposure::save(
                                        tx,
                                        previous.as_ref(),
                                        &next,
                                        &registry,
                                        at,
                                    )?;
                                }
                                context::insert_snapshot(tx, &frozen_context_snapshot)?;
                                checkpoint::freeze(
                                    tx,
                                    step.model_step_id,
                                    frozen_context_snapshot.id,
                                    at,
                                )?;
                                store::freeze_step_tool_settings(
                                    tx,
                                    step.model_step_id,
                                    &tool_settings,
                                    at,
                                )?;
                                model::insert_snapshot(
                                    tx,
                                    turn.session_id,
                                    &frozen_model_snapshot,
                                )?;
                                let mut next = step.clone();
                                next.state = ModelStepState::Running {
                                    execution_snapshot_id: frozen_model_snapshot.id,
                                };
                                turns::save_step(tx, &step, &next)?;
                                aura_persistence_v2::agent::inspector::freeze(
                                    tx,
                                    &prompt_inspector,
                                    at,
                                )?;
                                let mut next_run = run.clone();
                                next_run.revision = Revision::new(run.revision.value() + 1);
                                next_run.state = AgentRunState::ModelRunning {
                                    model_step_id: step.model_step_id,
                                };
                                next_run.updated_at = at;
                                turns::save_run(tx, &run, &next_run)?;
                                context::remediation::clear(tx, run_id, false, at)?;
                                model::context_window::freeze_trial(
                                    tx,
                                    &frozen_model_snapshot,
                                    window_trial.as_ref(),
                                    at,
                                )
                            },
                        )?)
                    },
                )
                .await?;
        }
        Ok(Some(Prepared {
            profile: model_profile,
            request: super::provider::Request {
                p05: self.p05.clone(),
                endpoint,
                store: Arc::clone(&self.store),
                identity,
                logical,
                snapshot,
                manifest: context.model_egress_manifest,
                attachments: attachments.into(),
                backup_waiting,
            },
        }))
    }
}

/// The conversation's newest counted request as a measurement for this one: only the same
/// model's, because another model's tokenizer counts the same text differently.
fn measured(
    counted: aura_persistence_v2::agent::counted_request::CountedRequest,
    now: &ModelExecutionSnapshot,
) -> Option<MeasuredRequest> {
    if counted.provider_id != now.provider_id || counted.model_name != now.model_name {
        return None;
    }
    MeasuredRequest::new(
        counted.input_tokens,
        counted.parts.into_iter().map(|part| (part.content_hash, part.byte_length)),
    )
}
