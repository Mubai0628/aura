//! Real Core prepare/permission/ledger/transport, with only the remote HTTP service substituted.
use super::*;
use provider_fixture::Scenario;

fn services(runtime: &crate::RuntimeState) -> &crate::RuntimeServices {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services
}

fn plain(body: &Value) -> String {
    fn texts(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(text) => out.push(text.clone()),
            Value::Array(values) => {
                for value in values {
                    texts(value, out);
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    texts(value, out);
                }
            }
            _ => {}
        }
    }
    let mut values = Vec::new();
    texts(body, &mut values);
    values.join("\n")
}

async fn prepare(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
    format: ModelApiFormat,
) -> (crate::RuntimeState, SessionId, WorkspaceId, ModelProfileId) {
    let model = super::activity_provider::configure_startup(host, &provider.url, format);
    let runtime = host.start().await;
    let root = host.root.path().join("activity-wire-workspace");
    std::fs::create_dir(&root).expect("actual activity wire fixture");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Wire"}),
    )
    .await;
    let workspace: WorkspaceId =
        serde_json::from_value(created["workspace"]["workspace_id"].clone())
            .expect("actual activity wire fixture");
    let session = create_session(&runtime, workspace, PermissionMode::ReadOnly).await;
    (runtime, session, workspace, model)
}

fn row(
    runtime: &crate::RuntimeState,
    session: SessionId,
    workspace: WorkspaceId,
    name: &str,
) -> aura_activity_domain::ActivityRecord {
    use aura_activity_domain::*;
    let owner = services(runtime).activity_runtime_instance_id;
    let execution = ActivityExecutionRef::MainFileMutation {
        operation_id: SecurityOperationId::new(),
        command_invocation_id: Some(CommandInvocationId::new()),
    };
    ActivityRecord {
        id: ActivityId::derive(ActivityKind::MainFs, &execution, 1),
        kind: ActivityKind::MainFs,
        execution,
        generation: 1,
        tool_id: ToolId::from_stable_name("fs.write_file"),
        tool_name: "fs.write_file".into(),
        tool_group: ActivityToolGroup::FileWrite,
        private_source: ActivityPrivateSource::WorkspaceMetadata,
        owner: ActivityOwnerScope {
            session_id: session,
            workspace_id: workspace,
            agent_run_id: None,
            turn_task_id: None,
            model_step_id: None,
            runtime_instance_id: owner,
        },
        resources: vec![ActivityResource {
            target: ActivityResourceTarget::VerifiedObject {
                identity: FileIdentity {
                    volume_serial_number: "18446744073709551614".into(),
                    file_id: "aabbccddeeff00112233445566778899".into(),
                },
                ancestors: Vec::new(),
                subtree: false,
            },
            relation: ActivityResourceRelation::Write,
            display_name: name.into(),
            display_name_truncated: false,
        }],
        parameters: vec![ActivityParameter {
            key: "path".into(),
            value: name.into(),
            truncated: false,
        }],
        started_at: UtcTimestamp::now(),
        runtime_instance_id: owner,
        published_at_revision: Revision::INITIAL,
    }
}

async fn drive(format: ModelApiFormat, retry: bool) {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Activity { format, retry }).await;
    let (runtime, session, workspace, model) = prepare(&host, &provider, format).await;
    let own = "own-path-\"quoted\"-</context>-data";
    let private = "other-session-private-path-canary";
    let index = &services(&runtime).activity_index;
    let mut own_row = row(&runtime, session, workspace, own);
    own_row.private_source = aura_activity_domain::ActivityPrivateSource::PersonalData;
    assert_eq!(index.publish(own_row), aura_activity_domain::ActivityPublishOutcome::Recorded);
    let other = create_session(&runtime, workspace, PermissionMode::ReadOnly).await;
    assert_eq!(
        index.publish(row(&runtime, other, workspace, private)),
        aura_activity_domain::ActivityPublishOutcome::Recorded
    );
    let run = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: run.run.agent_run_id };
    if retry {
        tokio::time::timeout(std::time::Duration::from_secs(15), provider.parked.notified())
            .await
            .expect("actual activity wire fixture");
        let revision = index.revision();
        assert_eq!(
            index.publish(row(&runtime, session, workspace, "new-activity-after-freeze")),
            aura_activity_domain::ActivityPublishOutcome::Recorded
        );
        assert!(index.revision() > revision);
        provider.release_window.notify_one();
    }
    let done =
        waiting::wait(&runtime, &request, |projection| projection.turn.state.terminal()).await;
    let diagnostics = {
        let store = services(&runtime).broker.lock().expect("actual activity wire fixture");
        let mut query = store.database.connection().prepare(
            "SELECT kind, payload FROM events WHERE kind IN ('agent.application_error','model.failure','tool.dispatch_adapter_error') ORDER BY rowid"
        ).expect("actual activity wire fixture");
        query
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .expect("actual activity wire fixture")
            .collect::<Result<Vec<_>, _>>()
            .expect("actual activity wire fixture")
    };
    assert!(
        matches!(done.turn.state, TurnTaskState::Completed { .. }),
        "actual query run completed: {:?}; captured {} requests; diagnostics={diagnostics:?}",
        done.turn.state,
        provider.requests.lock().expect("actual activity wire fixture").len()
    );
    let bodies = provider.requests.lock().expect("actual activity wire fixture").clone();
    assert!(
        bodies.len() >= 5 + usize::from(retry),
        "initial, tool continuation and Core continuation captured: {}",
        bodies.len()
    );
    for body in &bodies {
        let text = plain(body);
        assert!(text.contains("coverage="), "actual dynamic fragment reached {format:?}");
        assert!(
            !text.contains(private),
            "another real session's private fields were filtered before model egress"
        );
        assert!(text.contains("activity_id"));
    }
    if retry {
        let raw = provider.raw_requests.lock().expect("actual activity wire fixture");
        assert_eq!(
            raw[0], raw[1],
            "captured HTTP body bytes, not reserialized JSON, are identical"
        );
        assert_eq!(
            bodies[0], bodies[1],
            "real transport retry retains the frozen request despite the changed live index"
        );
    }
    let next = plain(&bodies[3 + usize::from(retry)]);
    assert!(
        next.contains("contract_version")
            && next.contains("page")
            && next.contains("activity_call"),
        "actual Agent query result reached the complete tool history group: {next}"
    );
    {
        let store = services(&runtime).broker.lock().expect("actual activity wire fixture");
        let db = store.database.connection();
        let failures: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='agent.application_error'",
                [],
                |row| row.get(0),
            )
            .expect("actual activity wire fixture");
        assert_eq!(failures, 0);
    }
    let run_id = request.agent_run_id;
    let (history, steps, logical_history) = services(&runtime)
        .agent_required()
        .store()
        .read(
            crate::agent_store::StoreStep::run(run_id, "activity.wire.verified_history"),
            move |db| {
                Ok((
                    aura_persistence_v2::context::history(db, session)?,
                    aura_persistence_v2::agent::model_context::steps(db, run_id)?,
                    // Continuations include this turn's verified tool results. The initial
                    // request intentionally excludes them and cannot prove their source class.
                    aura_persistence_v2::agent::model_context::history_for_model(
                        db, run_id, false,
                    )?
                    .items,
                ))
            },
        )
        .await
        .expect("actual activity wire fixture");
    let groups = history.iter().filter(|group| group.agent_run_id == run_id).collect::<Vec<_>>();
    assert_eq!(groups.len(), 3, "discovery, load and query each have a durable result group");
    assert!(groups.iter().all(|group| group.calls.len() == 1));
    let query_history = logical_history.iter().find(|item| {
        matches!(item, LogicalConversationItem::ToolResult { tool_name, .. } if tool_name == "agent.activity.query")
    }).expect("the actual query has a verified tool history result");
    let fragment = super::super::context::history_fragment(query_history)
        .expect("the intact query result retains its data class");
    assert_eq!(fragment.data_class, aura_security_contract::NetworkDataClass::PersonalData);
    let LogicalConversationItem::ToolResult { content, content_hash, .. } = query_history else {
        unreachable!("the query history item was matched above")
    };
    assert_eq!(fragment.content, *content);
    assert_eq!(ContentHash::sha256(fragment.content.as_bytes()), *content_hash);
    assert_eq!(
        fragment
            .data_class
            .model_channel_disposition(aura_security_contract::ModelEgressPolicyVersion::V1)
            .expect("the inherited policy version is supported"),
        aura_security_contract::ModelChannelDataDisposition::RequiresSourceEgressGrant,
        "PersonalData must not acquire WorkspaceSource's V1 bypass"
    );
    // The history holds the model's view of the result (`model_view`), whose outcome is the
    // envelope's own.
    #[derive(serde::Serialize)]
    struct View {
        outcome: aura_tool_contract::ModelVisibleToolResultOutcomeV1,
    }
    let stored: Value = serde_json::from_str(content).expect("the actual stored view");
    let mut envelope = View {
        outcome: serde_json::from_value(stored["outcome"].clone())
            .expect("the actual stored outcome is typed"),
    };
    let aura_tool_contract::ModelVisibleToolResultOutcomeV1::Succeeded {
        payload: aura_tool_contract::ModelVisibleToolSuccessPayloadV1::StructuredJson { value },
    } = &mut envelope.outcome
    else {
        panic!("the real query produced its structured result")
    };
    let mut metadata: aura_activity_domain::AgentActivityQueryResultV1 =
        serde_json::from_value(value.clone()).expect("typed actual query page");
    for row in &mut metadata.page.rows {
        row.private_source = aura_activity_domain::ActivityPrivateSource::WorkspaceMetadata;
    }
    *value = serde_json::to_value(&metadata).expect("metadata control");
    assert_eq!(
        super::super::activity::query_history_class(
            &serde_json::to_string(&envelope).expect("intact control envelope")
        )
        .expect("typed metadata query"),
        aura_security_contract::NetworkDataClass::WorkspaceMetadata
    );
    metadata.contract_version += 1;
    if let aura_tool_contract::ModelVisibleToolResultOutcomeV1::Succeeded {
        payload: aura_tool_contract::ModelVisibleToolSuccessPayloadV1::StructuredJson { value },
    } = &mut envelope.outcome
    {
        *value = serde_json::to_value(metadata).expect("unsupported version counterexample");
    }
    assert!(
        super::super::activity::query_history_class(
            &serde_json::to_string(&envelope).expect("counterexample envelope")
        )
        .is_err(),
        "an unsupported query body cannot silently acquire a less private class"
    );
    for item in &logical_history {
        if matches!(item, LogicalConversationItem::ToolResult { tool_name, .. } if tool_name != "agent.activity.query")
        {
            assert_eq!(
                super::super::context::history_fragment(item)
                    .expect("ordinary verified history")
                    .data_class,
                aura_security_contract::NetworkDataClass::WorkspaceSource,
                "the repair is limited to the new query result"
            );
        }
    }
    assert!(
        steps.iter().any(|step| matches!(step.continuation, AgentContinuation::ToolBatch { .. }))
    );
    assert!(
        steps
            .iter()
            .any(|step| matches!(step.continuation, AgentContinuation::CompletionReminder { .. })),
        "the captured last request really was a Core continuation"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_actual_query_continuations_privacy_and_mutating_retry() {
    drive(ModelApiFormat::OpenAiChatCompletions, true).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn responses_actual_query_continuations_and_privacy() {
    drive(ModelApiFormat::OpenAiResponses, false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn anthropic_actual_query_continuations_and_privacy() {
    drive(ModelApiFormat::AnthropicMessages, false).await;
}
