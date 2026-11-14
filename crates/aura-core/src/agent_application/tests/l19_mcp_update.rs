//! MCP tools after an update. A version that derives a tool's definition differently from the
//! same server settings and listing gives the tool the next revision, so its turns never meet the
//! definition an earlier version stored under the same revision.
use super::*;
use aura_tool_contract::{ToolAudienceV1, ToolVersion};
use aura_tool_domain::ToolDefinitionRecordV1;
use provider_fixture::Scenario;

/// The definition as it was derived before sub-agents could be given MCP tools.
fn derived_before(definition: &ToolDefinitionRecordV1) -> ToolDefinitionRecordV1 {
    let d = definition.clone();
    ToolDefinitionRecordV1::new(
        d.tool_id,
        d.tool_version,
        d.tool_package_id,
        d.package_key,
        d.tool_key,
        d.canonical_name,
        d.display_name,
        d.model_description,
        d.model_summary,
        d.input_schema,
        d.strict,
        d.result_contract,
        d.runtime,
        d.capabilities,
        d.resource_claims,
        ToolAudienceV1::ParentAgentOnly,
        d.source_kind,
        d.execution_semantics,
        d.runtime_file_closure_hash,
        d.entity_revision,
        d.created_at,
    )
    .expect("the earlier definition")
}

async fn completed_turn(
    runtime: &crate::RuntimeState,
    session: SessionId,
    model: ModelProfileId,
) -> AgentRunProjection {
    let started: AgentRunProjection = serde_json::from_value(
        crate::r03_tests::success(
            runtime,
            "agent.start",
            json!({
                "session_id": session,
                "model_profile_id": model,
                "text": "你好",
                "mode": "normal",
                "goal_budget": null,
            }),
        )
        .await,
    )
    .expect("run");
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    wait(runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await
}

#[tokio::test]
async fn an_mcp_tool_an_earlier_version_derived_differently_takes_the_next_revision() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::ReadAttachedFile).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    super::l16_mcp_resources::notes_server(&host, &runtime).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { unreachable!() };
    let server = services
        .mcp
        .configured()
        .servers
        .into_iter()
        .find(|server| server.id == "notes")
        .expect("the server");
    // What the earlier version left: revision 1 of each tool under the key it gave the tool's
    // inputs, and the definition it derived for that revision, which only the main agent could
    // be given.
    let tools = {
        let mut journal = services.broker.lock().expect("journal");
        let tools = crate::agent_execution::mcp_tools::usable_tools(
            &services.mcp,
            journal.database.connection(),
        );
        assert!(!tools.is_empty(), "the server's tools have their revisions");
        let earlier = tools
            .iter()
            .map(|tool| {
                let key = aura_domain_common::jcs::canonical_sha256(&json!({
                    "tool": tool.tool,
                    "config_fingerprint": tool.config_fingerprint,
                    "display_name": server.display_name,
                    "call_timeout_ms": server.call_timeout_ms,
                }))
                .expect("the earlier key");
                (key.hex().to_owned(), derived_before(&tool.definition), tool.instance.clone())
            })
            .collect::<Vec<_>>();
        journal
            .database
            .unit_of_work(|tx| -> aura_persistence_core::StoreResult<()> {
                for (key, definition, instance) in &earlier {
                    tx.execute(
                        "UPDATE mcp_tool_revisions SET definition_key=?1 \
                         WHERE tool_id=?2 AND revision=1",
                        rusqlite::params![key, definition.tool_id.to_string()],
                    )?;
                    aura_persistence_v2::tool::definitions::put(tx, definition, instance)?;
                }
                Ok(())
            })
            .expect("the earlier version's records");
        tools
    };

    // The first turn after the update runs; the tools get their next revision meanwhile.
    completed_turn(&runtime, session, model).await;
    let revised = |tool: &crate::agent_execution::mcp_tools::McpTool| {
        let key = aura_extension_service::mcp::identity::definition_key(
            &server,
            &tool.config_fingerprint,
            &tool.tool,
        )
        .expect("the key");
        let journal = services.broker.lock().expect("journal");
        aura_persistence_v2::mcp::revision_of(
            journal.database.connection(),
            &tool.definition.tool_id.to_string(),
            &key,
        )
        .expect("the revision")
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while tools.iter().any(|tool| revised(tool).is_none()) {
        assert!(std::time::Instant::now() < deadline, "the tools get their next revision");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(tools.iter().all(|tool| revised(tool).is_some_and(|r| r.revision == 2)));

    // The next turn has them under that revision, and the earlier definition stays as it was.
    completed_turn(&runtime, session, model).await;
    let journal = services.broker.lock().expect("journal");
    let db = journal.database.connection();
    for tool in &tools {
        let id = tool.definition.tool_id;
        let stored = |revision: u64| {
            let version = ToolVersion::new(None, Revision::new(revision)).expect("version");
            aura_persistence_v2::tool::definitions::get(db, id, &version).expect("definition")
        };
        assert_eq!(stored(1), Some(derived_before(&tool.definition)), "{}", tool.tool.wire_name);
        let next = stored(2).expect("the next revision is stored");
        assert_eq!(next.audience, ToolAudienceV1::SubagentConfigurable);
    }
}
