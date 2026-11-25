use super::prepare_step::{PreparedStep, StepPreparation};
use super::*;
use aura_agent_execution::agent_execution;
// The thinking level the conversation chose for its turn.
#[path = "reasoning.rs"]
mod reasoning;
// The speed mode the conversation chose for its turn.
#[path = "speed.rs"]
pub(crate) mod speed;
// The browser's screenshots a model that takes images is sent.
#[path = "screenshots.rs"]
pub mod screenshots;
// The capability modules each loaded tool brings to the prompt.
#[path = "prompt_capabilities.rs"]
mod prompt_capabilities;
// The workspace's AGENTS.md, read for each step.
#[path = "agents.rs"]
mod agents;
#[path = "prior_materials.rs"]
pub(in crate::agent_application) mod prior_materials;
#[path = "recall.rs"]
pub(in crate::agent_application) mod recall;
#[path = "team_downloads.rs"]
mod team_downloads;

impl AgentApplication {
    pub(super) async fn prepare_existing_step(
        self: &Arc<Self>,
        draft: &StepPreparation,
        cancel: &ModelCancellationToken,
    ) -> anyhow::Result<PreparedStep> {
        let step = draft.step.clone();
        let run_id = step.agent_run_id;
        let profile = draft.profile;
        let reasoning = draft.reasoning;
        let speed_mode = draft.speed_mode.clone();
        let permission = self.freeze_permission(draft.session_id).await?;
        let at = UtcTimestamp::now();
        let config = self.p05.model_config.snapshot()?;
        let model_profile = config
            .registry
            .models
            .get(&profile)
            .ok_or_else(|| anyhow::anyhow!("model.profile_missing"))?
            .clone();
        anyhow::ensure!(model_profile.enabled, "model.profile_disabled");
        // The provider's default connection, or the
        // next backup when the connections before it failed in this run's current streak.
        let order = aura_model_config::connection_order(&config.registry, &model_profile);
        let (failed, asked_again_after) = self
            .store
            .read(StoreStep::run(run_id, "agent.context.failed_connections"), move |db| {
                Ok((
                    aura_persistence_v2::model::transient_retry::failed_connections(db, run_id)?,
                    aura_persistence_v2::model::transient_retry::asked_again_after(db, run_id)?,
                ))
            })
            .await?;
        let idle_retry = asked_again_after.as_deref()
            == Some(aura_persistence_v2::model::transient_retry::STREAM_IDLE);
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
        let window = window_trial
            .as_ref()
            .and_then(|trial| trial.candidate_windows.last().copied())
            .unwrap_or(model_profile.context.runtime_effective_window_tokens);
        let reserve =
            trigger_output_reserve(model_profile.effective_max_output_tokens().unwrap_or(1));
        let history_budget = request_input_budget_v1(window, reserve).map_or_else(
            |_| window.saturating_sub(safety_reserve_v1(window).unwrap_or(window)),
            |(_, input)| input,
        );
        let (
            turn,
            run,
            bundle,
            prompt,
            settings,
            latest_input,
            registry,
            loads,
            tool_settings,
            package_tools,
            mut state,
        ) = self
            .assemble_frozen_facts(run_id, &permission, &step, at, self.computer_offered(profile))
            .await?;
        let member = super::super::team::team_member(self, turn.session_id).await?;
        let activity_summary = self.freeze_activity_summary(
            turn.workspace_id,
            turn.session_id,
            run_id,
            step.model_step_id,
        );
        debug_assert!(activity_summary.belongs_to_step(step.model_step_id));
        let activity_fragments =
            if member.is_none() { activity_summary.fragments()? } else { Vec::new() };
        let downloads = self
            .team_downloads(
                &turn,
                member.as_ref(),
                latest_input.as_ref().map(|(_, text)| text.as_str()),
            )
            .await;
        let memory_available = registry
            .catalog_entries
            .iter()
            .any(|entry| entry.canonical_name.as_str() == "memory.read");
        let source_access = super::compaction::dependencies::Access::for_step(
            &turn,
            latest_input.as_ref().map(|(id, text)| (*id, text.as_str())),
            memory_available,
            activity_fragments.iter().chain(downloads.iter()),
        );
        let (history, authorized_checkpoint) =
            Box::pin(self.authorized_history(&turn, &step, history_budget, source_access.clone()))
                .await?;
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
        // Its own speed mode, when the model's settings allow speed modes.
        snapshot.request_options.speed_mode = speed::turn_mode(speed_mode.as_ref(), &model_profile);
        if let Some(window) = window_trial.as_ref().and_then(|r| r.candidate_windows.last()) {
            // This is a newly allocated, still-unpublished operation snapshot.
            // A trial never edits the Profile or an earlier immutable snapshot.
            snapshot.runtime_effective_window_tokens = *window;
            snapshot.historical_snapshot_max_tokens =
                model_profile.context.summary_target_for_window(*window);
        }
        // The step's `context.before_build` and `model.before_call` Hooks, before anything
        // is frozen. A block ends the run with the Hook's name and reason on record.
        let hooks =
            self.step_hooks(run_id, &turn, step.model_step_id, &snapshot.request_options).await?;
        if hooks.blocked {
            self.interrupt(run_id, "hooks.step_blocked", false).await?;
            return Ok(PreparedStep::Waiting);
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
            let mut fragment = history_fragment(item)?;
            if state["current_user_root_event_id"].is_null()
                && fragment.source_ref
                    == (ContextSourceRef::Event { event_id: turn.root_user_event_id })
            {
                fragment.source_kind = ModelEgressSourceKind::PeerTask;
                fragment.authorization_basis = ModelEgressAuthorizationBasis::ModelChannelPolicy;
            }
            fragments.push(fragment);
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
        // Written again before each compilation pass, as the conversation then stands.
        let mut task_state_fragments = vec![ContextFragment {
            source_ref: ContextSourceRef::RuntimeState {
                state_kind: CONTROL_STATE.into(),
                state_revision: turn.revision,
            },
            source_kind: ModelEgressSourceKind::AuraCore,
            data_class: NetworkDataClass::PublicContent,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content: serde_json::to_string(&prompt_state(&state, &history.items))?,
        }];
        if registry
            .catalog_entries
            .iter()
            .any(|entry| entry.canonical_name.as_str() == "web.search")
        {
            let web = Arc::clone(&self.execution.web);
            let limits = tokio::task::spawn_blocking(move || web.web_search_limits()).await?;
            task_state_fragments.push(ContextFragment {
                source_ref: ContextSourceRef::RuntimeState {
                    state_kind: "agent.web_search_limits.v1".into(),
                    state_revision: turn.revision,
                },
                source_kind: ModelEgressSourceKind::AuraCore,
                data_class: NetworkDataClass::PublicContent,
                authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                content: serde_json::json!({"web_search_limits": limits}).to_string(),
            });
        }
        // A sub-agent sees its task sheet, the project's instructions and the project memory, and
        // the user's Skills when it was given their tools: not what other conversations are doing.
        if member.is_none() {
            task_state_fragments.push(self.multi_agent_fragment(turn.session_id).await?);
        }
        task_state_fragments.extend(activity_fragments);
        // The workspace's AGENTS.md, read again for every step: the folders' files with the task's
        // state, which changes anyway, the root file before the conversation.
        let agents =
            self.workspace_agents(run_id, turn.workspace_id, turn.revision, &history.items).await;
        task_state_fragments.extend(agents.folders);
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
        // The pinned Skills and the names of the others go first, then the root AGENTS.md. A
        // sub-agent in a worktree has the Skills of the workspace it was made from.
        let skills = member.as_ref().is_none_or(|member| {
            member
                .details
                .grant
                .tools
                .as_ref()
                .is_none_or(|tools| tools.iter().any(|tool| tool == "skills.load"))
        });
        let mut instructions = if skills {
            let workspace = self
                .execution
                .persistence
                .worktree_parent(turn.workspace_id)
                .unwrap_or(turn.workspace_id);
            self.turn_skill_fragments(run_id, workspace, turn.revision).await?
        } else {
            Vec::new()
        };
        instructions.extend(agents.root);
        sections.push(section(
            ContextSectionKind::WorkspaceAgentsInstructions,
            instructions,
            ContextCompressibility::NeverCompress,
        ));
        if !hooks.injections.is_empty() {
            let mut seen = std::collections::BTreeSet::new();
            sections.push(section(
                ContextSectionKind::HookInjections,
                hooks
                    .injections
                    .iter()
                    .filter(|text| seen.insert(text.as_str()))
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
                .flat_map(|s| prompt_capabilities::capabilities_of(&s.canonical_name.to_string()))
                .collect(),
            child_agent: member.is_some(),
            recovery_kinds: Default::default(),
            compaction: false,
            maintenance_kind: None,
        };
        let mut sources =
            assemble_prompt_context_sources(&prompt, &settings, &activation, sections)?;
        if let Some(core) = sources
            .section_inputs
            .iter_mut()
            .find(|section| section.kind == ContextSectionKind::CoreSystemPrompt)
        {
            core.fragments.push(ContextFragment {
                source_ref: ContextSourceRef::RuntimeState {
                    state_kind: "agent.checkpoint_rules.v1".into(),
                    state_revision: Revision::INITIAL,
                },
                source_kind: ModelEgressSourceKind::AuraCore,
                data_class: NetworkDataClass::PublicContent,
                authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                content: sources::CHECKPOINT_RULES.to_owned(),
            });
        }
        let estimators = super::super::estimators::for_model(profile);
        let (inspector_lineage, previous_snapshot, counted) = {
            // Boundary: one bounded read pair for the inspector view.
            let journal = Arc::clone(&self.execution.journal);
            let turn_task_id = turn.turn_task_id;
            let session_id = turn.session_id;
            let adopted_turn = turn.clone();
            let (provider, model) = (snapshot.provider_id, snapshot.model_name.clone());
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
                            aura_persistence_v2::agent::inspector::adopted_context(
                                db,
                                &adopted_turn,
                            )?,
                            // A new conversation borrows another one's count of this model.
                            aura_persistence_v2::agent::counted_request::for_model(
                                db, session_id, &provider, &model,
                            )?,
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
        if let Some(state) =
            input.section_inputs.iter_mut().find(|s| s.kind == ContextSectionKind::CurrentTaskState)
        {
            state.fragments.extend(downloads);
        }
        if let Some((start, end)) = history.deferred_range {
            recovery::record_gap(
                &mut input,
                &format!(
                    "{} older message groups between session sequences {start} and {end} are available on demand via history.list_turns, history.search and history.read.",
                    history.deferred_groups
                ),
            );
            recovery::clear_continuation(&mut input);
        }
        let background =
            Box::pin(super::compaction::apply(self, &mut input, authorized_checkpoint)).await?;
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
                input.previous_snapshot.as_ref(),
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
        let closing_question = asked_again_after.as_deref().and_then(super::provider::retry_note);
        let Some(compilation::CompiledStep { prepared, state: compiled_state, referenced_results }) =
            Box::pin(self.compile_context(compilation::CompilationInput {
                run_id,
                turn: &turn,
                step: &step,
                state,
                input,
                snapshot: &snapshot,
                closing_question,
                endpoint: &endpoint,
                window_trial: &window_trial,
                force_rebuild,
                history_read_pressure: history.compaction_pressure,
                selected_history_bytes: history.selected_history_bytes,
                missing_adopted: history.missing_adopted,
                cancel,
                estimators,
                fixed_tool_ids: &fixed_tool_ids,
                native_prefixes: &history.native_prefixes,
                screenshots: &screenshots,
                memory_available,
                source_access,
                background,
            }))
            .await?
        else {
            return Ok(PreparedStep::Waiting);
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
            // After everything the step sends, so the cut-off request's cache still serves it.
            closing_question: closing_question.map(str::to_owned),
            closing_exchange: Vec::new(),
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
        let native_prefix = prepared.native_history.map(|native| {
            if native.missing_evidence > 0 || native.mismatched > 0 {
                aura_core_base::append_core_log("context.native_chain_rebuilt", serde_json::json!({
                    "model_step_id": step.model_step_id, "prefix_unavailable": native.missing_evidence,
                    "prefix_or_route_mismatch": native.mismatched,
                }));
            }
            native.prefix
        });
        let request_boundary = aura_persistence_v2::agent::request_boundary::RequestBoundary::of(
            &logical,
            native_prefix,
        )?;
        let frozen_context_snapshot = context.snapshot.clone();
        let frozen_model_snapshot = snapshot.clone();
        let artifacts = prepared.artifacts;
        let prompt_inspector = prepared.prompt_inspector;
        let activity_sources = activity_summary.sources();
        let committed = {
            let app = Arc::clone(self);
            let journal = Arc::clone(&self.execution.journal);
            self.store
                .run_critical(
                    StoreStep::run(run_id, "agent.context.commit"),
                    StoreAccess::Write,
                    move || {
                        Ok(journal.lock()?.database.unit_of_work(
                            |tx| -> aura_persistence_core::StoreResult<bool> {
                                if inbox::pending_additions(tx, &turn)? {
                                    aura_persistence_v2::agent::input_consumption::additions(
                                        tx,
                                        run_id,
                                        UtcTimestamp::now(),
                                    )?;
                                    return Ok(false);
                                }
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
                                    return Ok(false);
                                }
                                let delivery =
                                    aura_persistence_v2::agent::peer_input::delivery(tx, &turn)?;
                                let latest_peers =
                                    app.peer_prompt_state(tx, &turn, delivery.as_ref()).map_err(
                                        |_| aura_persistence_core::PersistenceCoreError::Conflict,
                                    )?;
                                if latest_peers.as_ref() != state.get("peer_collaboration") {
                                    return Ok(false);
                                }
                                if let Some(boundary) = &request_boundary {
                                    boundary.freeze(tx, step.model_step_id, at)?;
                                }
                                for (call, original) in &referenced_results {
                                    context::read::index_tool_result(tx, *call, original, at)?;
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
                                agent_execution::package_tools::store(tx, &package_tools)?;
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
                                aura_persistence_v2::agent::inspector::freeze_activity_sources(
                                    tx,
                                    step.model_step_id,
                                    &activity_sources,
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
                                )?;
                                Ok(true)
                            },
                        )?)
                    },
                )
                .await?
        };
        if !committed {
            return Ok(PreparedStep::DraftChanged);
        }
        Ok(PreparedStep::Ready(Box::new(Prepared {
            profile: model_profile,
            request: super::provider::Request {
                single_attempt: false,
                p05: self.p05.clone(),
                endpoint,
                store: Arc::clone(&self.store),
                identity,
                logical,
                snapshot,
                manifest: context.model_egress_manifest,
                attachments: attachments.into(),
                backup_waiting,
                idle_retry,
            },
        })))
    }
}

/// A counted request (this conversation's, or before it has one another conversation's) as a
/// measurement for this one: only the same model's, because another model's tokenizer counts
/// the same text differently.
fn measured(
    counted: aura_persistence_v2::agent::counted_request::CountedRequest,
    now: &ModelExecutionSnapshot,
) -> Option<MeasuredRequest> {
    if counted.provider_id != now.provider_id
        || counted.model_name != now.model_name
        || counted.endpoint_id != now.endpoint_id
        || counted.endpoint_fingerprint != now.endpoint_fingerprint
    {
        return None;
    }
    MeasuredRequest::from_report(counted.input_tokens, counted.cached_input_tokens, &counted.parts)
}
